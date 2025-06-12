use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::core::*;

use kolme_store::KolmeStoreError;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::{BlockHeight, TxHash};

#[derive(Clone)]
pub struct KolmeStoreInMemory(Arc<tokio::sync::RwLock<Inner>>, Arc<tokio::sync::Semaphore>);

impl Default for KolmeStoreInMemory {
    fn default() -> Self {
        Self(Default::default(), Arc::new(Semaphore::new(1)))
    }
}

#[derive(Default)]
struct Inner {
    merkle: MerkleMemoryStore,
    blockhashes: BTreeMap<BlockHeight, BlockHash>,
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

    pub(crate) async fn delete_block(&self, height: BlockHeight) {
        let mut guard = self.0.write().await;
        guard.blocks.remove(&height);
    }

    pub(crate) async fn load_latest_block(&self) -> Result<Option<BlockHeight>, KolmeStoreError> {
        let guard = self.0.read().await;
        let Some((key, _)) = guard.blocks.last_key_value() else {
            return Ok(None);
        };
        Ok(Some(*key))
    }

    pub(crate) async fn load_block<App: KolmeApp>(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<
        kolme_store::StorableBlock<SignedBlock<App::Message>, super::FrameworkState, App::State>,
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

    // kolme#144 - Update signature to use the KolmeStoreError type
    pub(crate) async fn add_block<App: KolmeApp>(
        &self,
        merkle_manager: &MerkleManager,
        block: &kolme_store::StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>,
    ) -> Result<(), KolmeStoreError> {
        let height = BlockHeight(block.height);
        let txhash = TxHash(block.txhash);

        let mut guard = self.0.write().await;

        if let Some(existing_hash) = guard.blockhashes.get(&height) {
            if existing_hash.0 != block.blockhash {
                return Err(KolmeStoreError::ConflictBlockInDb {
                    height: height.0,
                    hash: existing_hash.0,
                });
            } else {
                return Err(KolmeStoreError::MatchingBlockAlreadyInserted { height: height.0 });
            }
        }

        if guard.txhashes.contains_key(&txhash) {
            return Err(KolmeStoreError::TxAlreadyInDb { txhash: txhash.0 });
        }

        guard.txhashes.insert(txhash, height);

        let hash = merkle_manager
            .save(&mut guard.merkle, block)
            .await
            .map_err(KolmeStoreError::custom)?;
        guard.blocks.insert(height, hash.hash);
        guard.blockhashes.insert(height, BlockHash(block.blockhash));

        Ok(())
    }

    pub(crate) async fn take_construct_lock(&self) -> OwnedSemaphorePermit {
        self.1.clone().acquire_owned().await.unwrap()
    }

    pub(super) async fn get_merkle_store(&self) -> MerkleMemoryStore {
        self.0.read().await.merkle.clone()
    }
}
