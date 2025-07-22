use crate::core::*;
use std::{num::NonZeroUsize, path::Path};

use kolme_store::sqlx::postgres::PgConnectOptions;
use kolme_store::sqlx::{pool::PoolOptions, Postgres};
use kolme_store::{
    KolmeBackingStore, KolmeConstructLock, KolmeStore as KolmeStoreInner, KolmeStoreError,
    StorableBlock,
};
use lru::LruCache;
use parking_lot::RwLock;
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

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn new_postgres(url: &str) -> Result<Self> {
        KolmeStoreInner::new_postgres(url)
            .await
            .map(KolmeStore::from)
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres_with_options(
        connect: PgConnectOptions,
        options: PoolOptions<Postgres>,
    ) -> Result<Self> {
        KolmeStoreInner::new_postgres_with_options(connect, options)
            .await
            .map(KolmeStore::from)
            .map_err(anyhow::Error::from)
    }

    pub fn new_fjall(dir: impl AsRef<Path>) -> Result<Self> {
        KolmeStoreInner::new_fjall(dir).map(KolmeStore::from)
    }

    pub fn new_in_memory() -> Self {
        KolmeStoreInner::new_in_memory().into()
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
        self.inner.clear_blocks().await
    }

    pub(crate) async fn take_construct_lock(&self) -> Result<KolmeConstructLock> {
        Ok(self.inner.take_construct_lock().await?)
    }

    pub(crate) async fn has_merkle_hash(
        &self,
        hash: Sha256Hash,
    ) -> Result<bool, MerkleSerialError> {
        // TODO consider if we should look at the merkle manager's cache first
        // for efficiency, probably yes
        self.inner.has_merkle_hash(hash).await
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

        self.inner.add_merkle_layer(hash, layer).await
    }

    pub(crate) async fn archive_block(&self, height: BlockHeight) -> Result<()> {
        self.inner.archive_block(height.0).await
    }

    pub(crate) async fn get_latest_archived_block_height(&self) -> Result<Option<u64>> {
        self.inner.get_latest_archived_block_height().await
    }
}

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn load_latest_block(&self) -> Result<Option<BlockHeight>> {
        Ok(self.inner.load_latest_block().await?.map(BlockHeight))
    }

    pub async fn load_block(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<Option<StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>> {
        if let Some(storable) = self.block_cache.read().peek(&height) {
            return Ok(Some(storable.clone()));
        }

        Ok(self.inner.load_block(merkle_manager, height.0).await?)
    }

    pub async fn has_block(&self, height: BlockHeight) -> Result<bool, KolmeStoreError> {
        if self.block_cache.read().peek(&height).is_some() {
            return Ok(true);
        }

        self.inner.has_block(height.0).await
    }

    pub async fn load_signed_block(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<Option<Arc<SignedBlock<App::Message>>>> {
        if let Some(storable) = self.block_cache.read().peek(&height) {
            return Ok(Some(storable.block.clone()));
        }

        Ok(self
            .inner
            .load_signed_block::<SignedBlock<App::Message>, FrameworkState, App::State>(
                merkle_manager,
                height.0,
            )
            .await?)
    }

    pub(super) async fn get_height_for_tx(&self, txhash: TxHash) -> Result<Option<BlockHeight>> {
        Ok(self
            .inner
            .get_height_for_tx(txhash.0)
            .await?
            .map(BlockHeight))
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

        let insertion_result = self.inner.add_block(merkle_manager, &block).await;
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
        let actual = self.inner.save(merkle_manager, value).await?;
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
        self.inner.load(merkle_manager, hash).await
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
        self.inner.get_merkle_layer(hash).await
    }
}
