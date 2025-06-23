use std::path::Path;

use crate::core::*;

use kolme_store::{KolmeStoreError, StorableBlock};
use merkle_store::{FjallBlock, KolmeMerkleStore, Not};
use merkle_store_fjall::MerkleFjallStore;

use super::BlockHeight;

#[derive(Clone)]
pub struct KolmeStoreFjall {
    pub(super) fjall_block: FjallBlock,
    pub(super) merkle: KolmeMerkleStore,
}

impl KolmeStoreFjall {
    pub fn new(fjall_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let merkle = KolmeMerkleStore::new_fjall(fjall_dir)?;
        let fjall_block = FjallBlock::try_from_fjall_merkle_store(&merkle, None)?;

        Ok(Self {
            merkle,
            fjall_block,
        })
    }

    /// Create a Fjall store with the provided merkle store
    ///
    /// **NOTE**: If you were previously using `new` and are now migrating to this method, please make
    /// sure that `fjall_dir` is the same `fjall_dir` previously used on `new`.
    pub fn new_with_merkle_store_and_fjall_options<Store>(
        merkle: Store,
        fjall_dir: impl AsRef<Path>,
    ) -> anyhow::Result<Self>
    where
        Store: Into<KolmeMerkleStore> + Not<MerkleFjallStore>,
    {
        let merkle = merkle.into();
        let fjall_block = FjallBlock::try_from_options("merkle", fjall_dir.as_ref())?;

        Ok(Self {
            merkle,
            fjall_block,
        })
    }

    pub fn load_latest_block(&self) -> Result<Option<BlockHeight>, KolmeStoreError> {
        let Some(latest) = self.fjall_block.prefix("block:").next_back() else {
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
            .fjall_block
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
    ) -> Result<(), KolmeStoreError> {
        let key = block_key(BlockHeight(block.height));
        let contents = merkle_manager
            .serialize(block)
            .map_err(KolmeStoreError::custom)?;

        if let Some(existing_hash) = self.fjall_block.get(key).map_err(KolmeStoreError::custom)? {
            if existing_hash != contents.hash.as_array() {
                return Err(KolmeStoreError::ConflictingBlockInDb {
                    height: block.height,
                    hash: Sha256Hash::from_hash(&existing_hash).map_err(KolmeStoreError::custom)?,
                });
            } else {
                return Err(KolmeStoreError::MatchingBlockAlreadyInserted {
                    height: block.height,
                });
            }
        }

        let mut store = self.merkle.clone();
        merkle_manager
            .save_merkle_contents(&mut store, &contents)
            .await?;

        self.fjall_block
            .insert(key, contents.hash.as_array())
            .map_err(KolmeStoreError::custom)?;
        self.fjall_block
            .insert(tx_key(TxHash(block.txhash)), block.height.to_be_bytes())
            .map_err(KolmeStoreError::custom)?;
        self.fjall_block
            .persist(fjall::PersistMode::SyncAll)
            .map_err(KolmeStoreError::custom)?;

        Ok(())
    }

    pub fn get_height_for_tx(&self, txhash: TxHash) -> Result<Option<BlockHeight>> {
        let Some(height) = self.fjall_block.get(tx_key(txhash))? else {
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
            .fjall_block
            .first_key_value()
            .map_err(KolmeStoreError::custom)?
        {
            self.fjall_block
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
