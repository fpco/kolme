use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use kolme_store::KolmeStoreError;
use merkle_map::{MerkleDeserialize, MerkleManager, MerkleMemoryStore, Sha256Hash};

use super::{BlockHeight, TxHash};

#[derive(Default, Clone)]
pub struct KolmeStoreInMemory(Arc<tokio::sync::RwLock<Inner>>);

#[derive(Default)]
struct Inner {
    merkle: MerkleMemoryStore,
    blocks: BTreeMap<BlockHeight, Sha256Hash>,
    txhashes: HashMap<TxHash, BlockHeight>,
}

impl KolmeStoreInMemory {
    pub(crate) async fn clear_blocks(&self) -> Result<(), kolme_store::KolmeStoreError> {
        let mut guard = self.0.write().await;
        guard.blocks.clear();
        guard.txhashes.clear();
        Ok(())
    }

    pub(crate) async fn load_latest_block<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<kolme_store::StorableBlock<super::FrameworkState, AppState>>, KolmeStoreError>
    {
        let mut guard = self.0.write().await;
        let Some((_, hash)) = guard.blocks.last_key_value() else {
            return Ok(None);
        };
        let hash = *hash;

        merkle_manager
            .load(&mut guard.merkle, hash)
            .await
            .map(Some)
            .map_err(KolmeStoreError::custom)
    }

    pub(crate) async fn load_block<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<
        kolme_store::StorableBlock<super::FrameworkState, AppState>,
        kolme_store::KolmeStoreError,
    > {
        let mut guard = self.0.write().await;
        let Some(hash) = guard.blocks.get(&height) else {
            return Err(KolmeStoreError::BlockNotFound { height: height.0 });
        };
        let hash = *hash;

        merkle_manager
            .load(&mut guard.merkle, hash)
            .await
            .map_err(KolmeStoreError::custom)
    }

    pub(crate) async fn get_height_for_tx(
        &self,
        txhash: super::TxHash,
    ) -> Result<Option<BlockHeight>, anyhow::Error> {
        Ok(self.0.read().await.txhashes.get(&txhash).copied())
    }

    pub(crate) async fn add_block<AppState: merkle_map::MerkleSerialize>(
        &self,
        merkle_manager: &MerkleManager,
        block: &kolme_store::StorableBlock<super::FrameworkState, AppState>,
    ) -> Result<(), anyhow::Error> {
        let mut guard = self.0.write().await;

        let height = BlockHeight(block.height);
        let txhash = TxHash(block.txhash);

        if guard.blocks.contains_key(&height) {
            return Err(KolmeStoreError::BlockAlreadyInDb { height: height.0 }.into());
        }
        if guard.txhashes.contains_key(&txhash) {
            return Err(KolmeStoreError::TxAlreadyInDb { txhash: txhash.0 }.into());
        }

        guard.txhashes.insert(txhash, height);

        let hash = merkle_manager.save(&mut guard.merkle, block).await?;
        guard.blocks.insert(height, hash.hash);
        Ok(())
    }
}
