mod fjall;
mod in_memory;

use std::{num::NonZeroUsize, path::Path};

use crate::core::*;

use fjall::KolmeStoreFjall;
use in_memory::KolmeStoreInMemory;
use kolme_store::{KolmeStoreError, StorableBlock};
use kolme_store_postgresql::{
    sqlx::{pool::PoolOptions, Postgres},
    ConstructLock, KolmeStorePostgres, KolmeStorePurePostgres,
};
use lru::LruCache;
use parking_lot::RwLock;
use tokio::sync::OwnedSemaphorePermit;
use utils::trigger::{Trigger, TriggerSubscriber};

const BLOCK_CACHE_SIZE: usize = 60;

#[derive(Clone)]
pub struct KolmeStore<App: KolmeApp> {
    inner: KolmeStoreInner,
    block_cache: Arc<RwLock<BlockCacheMap<App>>>,
    notify: Trigger,
}

#[allow(type_alias_bounds)]
type BlockCacheMap<App: KolmeApp> =
    LruCache<BlockHeight, StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>;

#[derive(Clone)]
enum KolmeStoreInner {
    Fjall(KolmeStoreFjall),
    Postgres(KolmeStorePostgres),
    InMemory(in_memory::KolmeStoreInMemory),
    PurePostgres(KolmeStorePurePostgres),
}

impl<App: KolmeApp> From<KolmeStoreInner> for KolmeStore<App> {
    fn from(inner: KolmeStoreInner) -> Self {
        let cache = BlockCacheMap::<App>::new(NonZeroUsize::new(BLOCK_CACHE_SIZE).unwrap());

        Self {
            inner,
            block_cache: Arc::new(RwLock::new(cache)),
            notify: Trigger::new("notify"),
        }
    }
}

pub enum KolmeConstructLock {
    NoLocking,
    Postgres { _lock: ConstructLock },
    InMemory { _permit: OwnedSemaphorePermit },
}

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn new_postgres_with_fjall(url: &str, fjall_dir: impl AsRef<Path>) -> Result<Self> {
        KolmeStorePostgres::new(url, fjall_dir)
            .await
            .map(|x| KolmeStoreInner::Postgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres(url: &str) -> Result<Self> {
        KolmeStorePurePostgres::new(url)
            .await
            .map(|x| KolmeStoreInner::PurePostgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres_with_options(
        url: &str,
        options: PoolOptions<Postgres>,
    ) -> Result<Self> {
        KolmeStorePurePostgres::new_with_options(url, options)
            .await
            .map(|x| KolmeStoreInner::PurePostgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub fn new_fjall(dir: impl AsRef<Path>) -> Result<Self> {
        KolmeStoreFjall::new(dir).map(|x| KolmeStoreInner::Fjall(x).into())
    }

    pub fn new_in_memory() -> Self {
        KolmeStoreInner::InMemory(KolmeStoreInMemory::default()).into()
    }

    /// Ensures that either we have no blocks yet, or the first block has matching genesis info.
    pub(super) async fn validate_genesis_info(
        &self,
        merkle_manager: &MerkleManager,
        expected: &GenesisInfo,
    ) -> Result<()> {
        if let Some(actual) = self.load_genesis_info(merkle_manager).await? {
            anyhow::ensure!(
                &actual == expected,
                "Mismatched genesis info.\nActual:   {actual:?}\nExpected: {expected:?}"
            );
        }
        Ok(())
    }

    async fn load_genesis_info(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<GenesisInfo>> {
        let Some(block) = self
            .load_signed_block(merkle_manager, BlockHeight::start())
            .await?
        else {
            return Ok(None);
        };
        let messages = &block.tx().0.message.as_inner().messages;
        anyhow::ensure!(messages.len() == 1);
        match messages.first().unwrap() {
            Message::Genesis(genesis_info) => Ok(Some(genesis_info.clone())),
            _ => Err(anyhow::anyhow!("Invalid messages in first block")),
        }
    }

    /// Very dangerous function! Intended for testing. Deletes all blocks in the database.
    pub async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        self.block_cache.write().clear();
        match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                kolme_store_postgres.clear_blocks().await
            }
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => {
                kolme_store_postgres.clear_blocks().await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory.clear_blocks().await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => kolme_store_fjall.clear_blocks(),
        }
    }

    /// Delete the given block height.
    ///
    /// Currently only implemented for the in-memory store, since this is intended
    /// exclusively for test cases.
    pub async fn delete_block(&self, height: BlockHeight) -> Result<()> {
        match &self.inner {
            KolmeStoreInner::Fjall(_)
            | KolmeStoreInner::Postgres(_)
            | KolmeStoreInner::PurePostgres(_) => Err(anyhow::anyhow!(
                "KolmeStore::delete_block only supports InMemory"
            )),
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                self.block_cache.write().pop(&height);
                kolme_store_in_memory.delete_block(height).await;
                Ok(())
            }
        }
    }

    pub(crate) async fn take_construct_lock(&self) -> Result<KolmeConstructLock> {
        match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => Ok(KolmeConstructLock::Postgres {
                _lock: kolme_store_postgres.take_construct_lock().await?,
            }),
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => {
                Ok(KolmeConstructLock::Postgres {
                    _lock: kolme_store_postgres.take_construct_lock().await?,
                })
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => Ok(KolmeConstructLock::InMemory {
                _permit: kolme_store_in_memory.take_construct_lock().await,
            }),
            KolmeStoreInner::Fjall(_kolme_store_fjall) => Ok(KolmeConstructLock::NoLocking),
        }
    }

    pub(crate) async fn has_merkle_hash(
        &self,
        hash: Sha256Hash,
    ) -> Result<bool, MerkleSerialError> {
        // TODO consider if we should look at the merkle manager's cache first
        // for efficiency, probably yes
        match &self.inner {
            KolmeStoreInner::Fjall(store) => {
                let mut merkle = store.merkle.clone();
                merkle.contains_hash(hash).await
            }
            KolmeStoreInner::Postgres(store) => {
                let mut merkle = store.get_merkle_store().clone();
                merkle.contains_hash(hash).await
            }
            KolmeStoreInner::InMemory(store) => {
                let mut merkle = store.get_merkle_store().await;
                merkle.contains_hash(hash).await
            }
            KolmeStoreInner::PurePostgres(store) => {
                let mut merkle = store.new_store();
                merkle.contains_hash(hash).await
            }
        }
    }

    pub(crate) async fn add_merkle_layer(
        &self,
        hash: Sha256Hash,
        layer: &MerkleLayerContents,
    ) -> Result<()> {
        anyhow::ensure!(hash == Sha256Hash::hash(&layer.payload));
        for child in &layer.children {
            anyhow::ensure!(self.has_merkle_hash(*child).await?);
        }
        match &self.inner {
            KolmeStoreInner::Fjall(store) => {
                let mut merkle = store.merkle.clone();
                merkle.save_by_hash(hash, layer).await
            }
            KolmeStoreInner::Postgres(store) => {
                let mut merkle = store.get_merkle_store().clone();
                merkle.save_by_hash(hash, layer).await
            }
            KolmeStoreInner::InMemory(store) => {
                let mut merkle = store.get_merkle_store().await;
                merkle.save_by_hash(hash, layer).await
            }
            KolmeStoreInner::PurePostgres(store) => {
                let mut merkle = store.new_store();
                merkle.save_by_hash(hash, layer).await
            }
        }
        .map_err(anyhow::Error::from)
    }
}

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn load_latest_block(&self) -> Result<Option<BlockHeight>> {
        Ok(match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => kolme_store_postgres
                .load_latest_block()
                .map(|x| x.map(BlockHeight))?,
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => kolme_store_postgres
                .load_latest_block()
                .map(|x| x.map(BlockHeight))?,
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory.load_latest_block().await?
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => kolme_store_fjall.load_latest_block()?,
        })
    }

    pub async fn load_block(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<Option<StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>> {
        if let Some(storable) = self.block_cache.read().peek(&height) {
            return Ok(Some(storable.clone()));
        }

        let res = match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                kolme_store_postgres
                    .load_block(merkle_manager, height.0)
                    .await
            }
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => {
                kolme_store_postgres
                    .load_block(merkle_manager, height.0)
                    .await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory
                    .load_block::<App>(merkle_manager, height)
                    .await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                kolme_store_fjall
                    .load_block::<App>(merkle_manager, height)
                    .await
            }
        };
        match res {
            Err(KolmeStoreError::BlockNotFound { height: _ }) => Ok(None),
            Err(e) => Err(e.into()),
            Ok(block) => Ok(Some(block)),
        }
    }

    pub async fn has_block(&self, height: BlockHeight) -> Result<bool, KolmeStoreError> {
        if self.block_cache.read().peek(&height).is_some() {
            return Ok(true);
        }

        match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                kolme_store_postgres.has_block(height.0).await
            }
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => {
                kolme_store_postgres.has_block(height.0).await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory.has_block(height).await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => kolme_store_fjall.has_block(height),
        }
    }

    pub async fn load_signed_block(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<Option<Arc<SignedBlock<App::Message>>>> {
        if let Some(storable) = self.block_cache.read().peek(&height) {
            return Ok(Some(storable.block.clone()));
        }
        let res = match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => kolme_store_postgres
                .load_rendered_block(height.0)
                .await
                .and_then(|s| {
                    serde_json::from_str(&s)
                        .map_err(KolmeStoreError::custom)
                        .map(Arc::new)
                }),
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => kolme_store_postgres
                .load_rendered_block(height.0)
                .await
                .and_then(|s| {
                    serde_json::from_str(&s)
                        .map_err(KolmeStoreError::custom)
                        .map(Arc::new)
                }),
            KolmeStoreInner::InMemory(kolme_store_in_memory) => kolme_store_in_memory
                .load_block::<App>(merkle_manager, height)
                .await
                .map(|x| x.block),
            KolmeStoreInner::Fjall(kolme_store_fjall) => kolme_store_fjall
                .load_block::<App>(merkle_manager, height)
                .await
                .map(|x| x.block),
        };
        match res {
            Err(KolmeStoreError::BlockNotFound { height: _ }) => Ok(None),
            Err(e) => Err(e.into()),
            Ok(block) => Ok(Some(block)),
        }
    }

    pub(super) async fn get_height_for_tx(&self, txhash: TxHash) -> Result<Option<BlockHeight>> {
        match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => kolme_store_postgres
                .get_height_for_tx(txhash.0)
                .await
                .map(|x| x.map(BlockHeight))
                .map_err(anyhow::Error::from),
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => kolme_store_postgres
                .get_height_for_tx(txhash.0)
                .await
                .map(|x| x.map(BlockHeight))
                .map_err(anyhow::Error::from),
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory.get_height_for_tx(txhash).await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                kolme_store_fjall.get_height_for_tx(txhash)
            }
        }
    }

    pub(super) async fn add_block(
        &self,
        merkle_manager: &MerkleManager,
        block: StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>,
    ) -> Result<()> {
        let inner = block.block.0.message.as_inner();
        self.save(
            merkle_manager,
            &block.framework_state,
            inner.framework_state,
        )
        .await?;
        self.save(merkle_manager, &block.app_state, inner.app_state)
            .await?;
        self.save(merkle_manager, &block.logs, inner.logs).await?;

        let insertion_result = match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                kolme_store_postgres.add_block(merkle_manager, &block).await
            }
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => {
                kolme_store_postgres.add_block(merkle_manager, &block).await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory
                    .add_block::<App>(merkle_manager, &block)
                    .await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                kolme_store_fjall
                    .add_block::<App>(merkle_manager, &block)
                    .await
            }
        };

        match insertion_result {
            Err(KolmeStoreError::MatchingBlockAlreadyInserted { .. }) | Ok(_) => {
                self.block_cache
                    .write()
                    .put(BlockHeight(block.height), block);

                self.trigger_notify();

                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Save data to the merkle store.
    pub(super) async fn save<T: MerkleSerializeRaw>(
        &self,
        merkle_manager: &MerkleManager,
        value: &T,
        expected: Sha256Hash,
    ) -> Result<()> {
        let actual = match &self.inner {
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                let mut store = kolme_store_fjall.merkle.clone();
                merkle_manager.save(&mut store, value).await?
            }
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                let mut store = kolme_store_postgres.get_merkle_store().clone();
                merkle_manager.save(&mut store, value).await?
            }
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => {
                kolme_store_postgres.save(merkle_manager, value).await?
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                let mut store = kolme_store_in_memory.get_merkle_store().await;
                merkle_manager.save(&mut store, value).await?
            }
        };
        anyhow::ensure!(
            expected == actual.hash,
            "Hash mismatch, expected {expected} but received {}",
            actual.hash
        );
        Ok(())
    }

    /// Load data from the merkle store.
    pub(super) async fn load<T: MerkleDeserializeRaw>(
        &self,
        merkle_manager: &MerkleManager,
        hash: Sha256Hash,
    ) -> Result<T, MerkleSerialError> {
        match &self.inner {
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                let mut store = kolme_store_fjall.merkle.clone();
                merkle_manager.load(&mut store, hash).await
            }
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                let mut store = kolme_store_postgres.get_merkle_store().clone();
                merkle_manager.load(&mut store, hash).await
            }
            KolmeStoreInner::PurePostgres(kolme_store_postgres) => {
                kolme_store_postgres.load(merkle_manager, hash).await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                let mut store = kolme_store_in_memory.get_merkle_store().await;
                merkle_manager.load(&mut store, hash).await
            }
        }
    }

    fn trigger_notify(&self) {
        self.notify.trigger();
    }

    /// Subscribe to receive notifications of new data becoming available in the store.
    pub(crate) fn subscribe(&self) -> TriggerSubscriber {
        self.notify.subscribe()
    }

    pub(crate) async fn get_merkle_layer(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleLayerContents>, MerkleSerialError> {
        match &self.inner {
            KolmeStoreInner::Fjall(store) => {
                let mut merkle = store.merkle.clone();
                merkle.load_by_hash(hash).await
            }
            KolmeStoreInner::Postgres(store) => {
                let mut merkle = store.get_merkle_store().clone();
                merkle.load_by_hash(hash).await
            }
            KolmeStoreInner::InMemory(store) => {
                let mut merkle = store.get_merkle_store().await;
                merkle.load_by_hash(hash).await
            }
            KolmeStoreInner::PurePostgres(store) => {
                let mut merkle = store.new_store();
                merkle.load_by_hash(hash).await
            }
        }
    }
}
