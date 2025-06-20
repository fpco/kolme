mod fjall;
mod in_memory;

use std::{num::NonZeroUsize, path::Path};

use crate::core::*;

use fjall::KolmeStoreFjall;
use in_memory::KolmeStoreInMemory;
use kolme_store::{KolmeStoreError, StorableBlock};
use kolme_store_postgresql::{ConstructLock, KolmeStorePostgres, sqlx::{pool::PoolOptions, Postgres}};
use lru::LruCache;
use merkle_store::{KolmeMerkleStore, Not};
use merkle_store_fjall::MerkleFjallStore;
use parking_lot::RwLock;
use tokio::sync::OwnedSemaphorePermit;

const BLOCK_CACHE_SIZE: usize = 60;

#[derive(Clone)]
pub struct KolmeStore<App: KolmeApp> {
    inner: KolmeStoreInner,
    block_cache: Arc<RwLock<BlockCacheMap<App>>>,
    notify: tokio::sync::watch::Sender<usize>,
}

#[allow(type_alias_bounds)]
type BlockCacheMap<App: KolmeApp> =
    LruCache<BlockHeight, StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>;

#[derive(Clone)]
enum KolmeStoreInner {
    Fjall(KolmeStoreFjall),
    Postgres(KolmeStorePostgres),
    InMemory(in_memory::KolmeStoreInMemory),
}

impl<App: KolmeApp> From<KolmeStoreInner> for KolmeStore<App> {
    fn from(inner: KolmeStoreInner) -> Self {
        let cache = BlockCacheMap::<App>::new(NonZeroUsize::new(BLOCK_CACHE_SIZE).unwrap());
        let notify = tokio::sync::watch::Sender::new(0);

        Self {
            inner,
            block_cache: Arc::new(RwLock::new(cache)),
            notify,
        }
    }
}

pub enum KolmeConstructLock {
    NoLocking,
    Postgres { _lock: ConstructLock },
    InMemory { _permit: OwnedSemaphorePermit },
}

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn new_postgres(url: &str, fjall_dir: impl AsRef<Path>) -> Result<Self> {
        KolmeStorePostgres::new(url, fjall_dir)
            .await
            .map(|x| KolmeStoreInner::Postgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres_with_options(url: &str, options: PoolOptions<Postgres>, fjall_dir: impl AsRef<Path>) -> Result<Self> {
        KolmeStorePostgres::new_with_options(url, options, fjall_dir)
            .await
            .map(|x| KolmeStoreInner::Postgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres_with_merkle_and_fjall<Store>(
        url: &str,
        store: Store,
        fjall_dir: impl AsRef<Path>,
    ) -> Result<Self>
    where
        Store: Into<KolmeMerkleStore> + Not<MerkleFjallStore>,
    {
        KolmeStorePostgres::new_with_merkle_and_fjall(url, store, fjall_dir)
            .await
            .map(|x| KolmeStoreInner::Postgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres_all_settings<Store>(
        url: &str,
        options: PoolOptions<Postgres>,
        store: Store,
        fjall_dir: impl AsRef<Path>,
    ) -> Result<Self>
    where
        Store: Into<KolmeMerkleStore> + Not<MerkleFjallStore>,
    {
        KolmeStorePostgres::new_all_settings(url, options, store, fjall_dir)
            .await
            .map(|x| KolmeStoreInner::Postgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub fn new_fjall(dir: impl AsRef<Path>) -> Result<Self> {
        KolmeStoreFjall::new(dir).map(|x| KolmeStoreInner::Fjall(x).into())
    }

    pub fn new_fjall_with_merkle_store_and_fjall_options<Store>(
        store: Store,
        dir: impl AsRef<Path>,
    ) -> Result<Self>
    where
        Store: Into<KolmeMerkleStore> + Not<MerkleFjallStore>,
    {
        KolmeStoreFjall::new_with_merkle_store_and_fjall_options(store, dir)
            .map(|x| KolmeStoreInner::Fjall(x).into())
    }

    pub fn new_in_memory() -> Self {
        KolmeStoreInner::InMemory(KolmeStoreInMemory::default()).into()
    }

    pub fn new_in_memory_with_store(merkle: KolmeMerkleStore) -> Self {
        KolmeStoreInner::InMemory(KolmeStoreInMemory::new_with_merkle_store(merkle)).into()
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
            KolmeStoreInner::Fjall(_) | KolmeStoreInner::Postgres(_) => Err(anyhow::anyhow!(
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
            KolmeStoreInner::InMemory(kolme_store_in_memory) => Ok(KolmeConstructLock::InMemory {
                _permit: kolme_store_in_memory.take_construct_lock().await,
            }),
            KolmeStoreInner::Fjall(_kolme_store_fjall) => Ok(KolmeConstructLock::NoLocking),
        }
    }
}

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn load_latest_block(&self) -> Result<Option<BlockHeight>> {
        Ok(match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => kolme_store_postgres
                .load_latest_block()
                .await
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
        let insertion_result = match &self.inner {
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
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

    /// Store merkle contents then reload the data as a serializable type.
    pub(super) async fn store_and_load<T: MerkleDeserializeRaw>(
        &self,
        merkle_manager: &MerkleManager,
        hash: Sha256Hash,
        contents: Arc<MerkleContents>,
    ) -> Result<T> {
        anyhow::ensure!(hash == contents.hash);

        let x = match &self.inner {
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                let mut store = kolme_store_fjall.merkle.clone();
                store_and_load_helper(merkle_manager, &mut store, &contents).await
            }
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                let mut store = kolme_store_postgres.get_merkle_store().clone();
                store_and_load_helper(merkle_manager, &mut store, &contents).await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                let mut store = kolme_store_in_memory.get_merkle_store().await;
                store_and_load_helper(merkle_manager, &mut store, &contents).await
            }
        }?;

        self.trigger_notify();
        Ok(x)
    }

    fn trigger_notify(&self) {
        self.notify.send_modify(|x| *x += 1);
    }

    /// Subscribe to receive notifications of new data becoming available in the store.
    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<usize> {
        self.notify.subscribe()
    }
}

async fn store_and_load_helper<T: MerkleDeserializeRaw, Store: MerkleStore>(
    merkle_manager: &MerkleManager,
    store: &mut Store,
    contents: &MerkleContents,
) -> Result<T> {
    merkle_manager.save_merkle_contents(store, contents).await?;
    merkle_manager
        .load(store, contents.hash)
        .await
        .map_err(Into::into)
}
