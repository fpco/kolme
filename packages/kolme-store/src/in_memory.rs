use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::{r#trait::KolmeBackingStore, KolmeConstructLock, KolmeStoreError, StorableBlock};
use merkle_map::{
    MerkleDeserialize, MerkleManager, MerkleMemoryStore, MerkleSerialize, MerkleStore, Sha256Hash,
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct Store(Arc<tokio::sync::RwLock<Inner>>, Arc<tokio::sync::Semaphore>);

impl Default for Store {
    fn default() -> Self {
        Self(Default::default(), Arc::new(Semaphore::new(1)))
    }
}

impl Store {
    async fn get_merkle_store(&self) -> MerkleMemoryStore {
        self.0.read().await.merkle.clone()
    }
}

#[derive(Default)]
struct Inner {
    merkle: MerkleMemoryStore,
    blockhashes: BTreeMap<u64, Sha256Hash>,
    blocks: BTreeMap<u64, Sha256Hash>,
    txhashes: HashMap<Sha256Hash, u64>,
    latest_archived_block: Option<u64>,
}

impl KolmeBackingStore for Store {
    async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        let mut guard = self.0.write().await;
        guard.blocks.clear();
        guard.txhashes.clear();
        Ok(())
    }

    async fn delete_block(&self, height: u64) -> Result<(), KolmeStoreError> {
        let mut guard = self.0.write().await;
        guard.blocks.remove(&height);
        Ok(())
    }

    async fn load_latest_block(&self) -> Result<Option<u64>, KolmeStoreError> {
        let guard = self.0.read().await;
        let Some((key, _)) = guard.blocks.last_key_value() else {
            return Ok(None);
        };
        Ok(Some(*key))
    }

    async fn load_block<Block, FrameworkState, AppState>(
        &self,
        merkle_manager: &MerkleManager,
        height: u64,
    ) -> Result<Option<StorableBlock<Block, FrameworkState, AppState>>, KolmeStoreError>
    where
        Block: serde::de::DeserializeOwned + MerkleDeserialize + MerkleSerialize,
        FrameworkState: MerkleDeserialize + MerkleSerialize,
        AppState: MerkleDeserialize + MerkleSerialize,
    {
        let mut guard = self.0.write().await;
        let Some(hash) = guard.blocks.get(&height) else {
            return Ok(None);
        };
        let hash = *hash;

        merkle_manager
            .load(&mut guard.merkle, hash)
            .await
            .map_err(KolmeStoreError::custom)
            .map(Some)
    }

    async fn has_block(&self, height: u64) -> Result<bool, KolmeStoreError> {
        Ok(self.0.read().await.blocks.contains_key(&height))
    }

    async fn get_height_for_tx(
        &self,
        txhash: Sha256Hash,
    ) -> std::result::Result<Option<u64>, KolmeStoreError> {
        Ok(self.0.read().await.txhashes.get(&txhash).copied())
    }

    async fn add_block<Block, FrameworkState, AppState>(
        &self,
        merkle_manager: &MerkleManager,
        block: &StorableBlock<Block, FrameworkState, AppState>,
    ) -> Result<(), KolmeStoreError>
    where
        Block: serde::Serialize + MerkleSerialize,
        FrameworkState: MerkleSerialize,
        AppState: MerkleSerialize,
    {
        let height = block.height;
        let txhash = block.txhash;

        let mut guard = self.0.write().await;

        if let Some(existing_hash) = guard.blockhashes.get(&height) {
            if existing_hash != &block.blockhash {
                return Err(KolmeStoreError::ConflictingBlockInDb {
                    height,
                    hash: *existing_hash,
                });
            } else {
                return Err(KolmeStoreError::MatchingBlockAlreadyInserted { height });
            }
        }

        if guard.txhashes.contains_key(&txhash) {
            return Err(KolmeStoreError::TxAlreadyInDb { txhash });
        }

        guard.txhashes.insert(txhash, height);

        let hash = merkle_manager
            .save(&mut guard.merkle, block)
            .await
            .map_err(KolmeStoreError::custom)?;
        guard.blocks.insert(height, hash.hash);
        guard.blockhashes.insert(height, block.blockhash);

        Ok(())
    }

    async fn take_construct_lock(&self) -> Result<KolmeConstructLock, KolmeStoreError> {
        Ok(KolmeConstructLock::InProcess {
            _lock: self.1.clone().acquire_owned().await.unwrap(),
        })
    }

    async fn get_merkle_layer(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<merkle_map::MerkleLayerContents>, merkle_map::MerkleSerialError> {
        let mut merkle = self.get_merkle_store().await;
        merkle.load_by_hash(hash).await
    }

    async fn has_merkle_hash(
        &self,
        hash: Sha256Hash,
    ) -> Result<bool, merkle_map::MerkleSerialError> {
        let mut merkle = self.get_merkle_store().await;
        merkle.contains_hash(hash).await
    }

    async fn add_merkle_layer(
        &self,
        hash: Sha256Hash,
        layer: &merkle_map::MerkleLayerContents,
    ) -> anyhow::Result<()> {
        let mut merkle = self.get_merkle_store().await;
        merkle.save_by_hash(hash, layer).await?;
        Ok(())
    }

    async fn save<T>(
        &self,
        merkle_manager: &MerkleManager,
        value: &T,
    ) -> anyhow::Result<Arc<merkle_map::MerkleContents>>
    where
        T: merkle_map::MerkleSerializeRaw,
    {
        let mut merkle = self.get_merkle_store().await;
        Ok(merkle_manager.save(&mut merkle, value).await?)
    }

    async fn load<T>(
        &self,
        merkle_manager: &MerkleManager,
        hash: Sha256Hash,
    ) -> Result<T, merkle_map::MerkleSerialError>
    where
        T: merkle_map::MerkleDeserializeRaw,
    {
        let mut merkle = self.get_merkle_store().await;
        merkle_manager.load(&mut merkle, hash).await
    }

    async fn archive_block(&self, height: u64) -> anyhow::Result<()> {
        self.0.write().await.latest_archived_block = Some(height);
        Ok(())
    }

    async fn get_latest_archived_block_height(&self) -> anyhow::Result<Option<u64>> {
        Ok(self.0.read().await.latest_archived_block)
    }
}
