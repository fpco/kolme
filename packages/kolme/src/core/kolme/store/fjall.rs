use std::path::Path;

use crate::core::*;

use kolme_store::{KolmeStoreError, StorableBlock};
use merkle_store_fjall::MerkleFjallStore;

use super::BlockHeight;

#[derive(Clone)]
pub struct KolmeStoreFjall {
    pub(super) merkle: MerkleFjallStore,
}

impl KolmeStoreFjall {
    pub fn new(fjall_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let merkle = MerkleFjallStore::new(fjall_dir)?;
        Ok(Self { merkle })
    }

    pub fn load_latest_block(&self) -> Result<Option<BlockHeight>, KolmeStoreError> {
        let Some(latest) = self.merkle.handle.prefix("block:").next_back() else {
            return Ok(None);
        };
        let (key, _hash_bytes) = latest.map_err(KolmeStoreError::custom)?;
        let key = (*key)
            .strip_prefix(b"block:")
            .ok_or_else(|| KolmeStoreError::Other("Fjall key missing block: prefix".to_owned()))?;
        let height = <[u8; 8]>::try_from(key).map_err(KolmeStoreError::custom)?;
        Ok(Some(BlockHeight(u64::from_be_bytes(height))))
    }

    pub async fn load_block<App: KolmeApp>(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>, KolmeStoreError>
    {
        let hash_bytes = self
            .merkle
            .handle
            .get(block_key(height))
            .map_err(KolmeStoreError::custom)?
            .ok_or(KolmeStoreError::BlockNotFound { height: height.0 })?;
        let hash = Sha256Hash::from_hash(&hash_bytes).map_err(KolmeStoreError::custom)?;
        let mut store = self.merkle.clone();
        merkle_manager
            .load(&mut store, hash)
            .await
            .map_err(KolmeStoreError::custom)
    }

    pub async fn add_block<App: KolmeApp>(
        &self,
        merkle_manager: &MerkleManager,
        block: &StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>,
    ) -> Result<()> {
        let key = block_key(BlockHeight(block.height));
        if self.merkle.handle.contains_key(key)? {
            Err(KolmeStoreError::BlockAlreadyInDb {
                height: block.height,
            }
            .into())
        } else {
            let mut store = self.merkle.clone();
            let contents = merkle_manager.save(&mut store, block).await?;

            // TODO do we worry about race conditions?
            self.merkle.handle.insert(key, contents.hash.as_array())?;
            self.merkle
                .handle
                .insert(tx_key(TxHash(block.txhash)), block.height.to_be_bytes())?;
            self.merkle.keyspace.persist(fjall::PersistMode::SyncAll)?;
            Ok(())
        }
    }

    pub fn get_height_for_tx(&self, txhash: TxHash) -> Result<Option<BlockHeight>> {
        let Some(height) = self.merkle.handle.get(tx_key(txhash))? else {
            return Ok(None);
        };
        let height = match <[u8; 8]>::try_from(&*height) {
            Ok(height) => BlockHeight(u64::from_be_bytes(height)),
            Err(e) => anyhow::bail!("get_height_for_tx: invalid height in Fjall store: {e}"),
        };
        Ok(Some(height))
    }

    pub fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        while let Some((key, _)) = self
            .merkle
            .handle
            .first_key_value()
            .map_err(KolmeStoreError::custom)?
        {
            self.merkle
                .handle
                .remove(key)
                .map_err(KolmeStoreError::custom)?;
        }
        Ok(())
    }
}

fn block_key(height: BlockHeight) -> [u8; 14] {
    let mut array = [b'b', b'l', b'o', b'c', b'k', b':', 0, 0, 0, 0, 0, 0, 0, 0];
    array[6..].copy_from_slice(&height.0.to_be_bytes());
    array
}

fn tx_key(tx: TxHash) -> [u8; 35] {
    let mut array = [0; 35];
    array[0..3].copy_from_slice(b"tx:");
    array[3..].copy_from_slice(tx.0.as_array());
    array
}
