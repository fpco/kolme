use std::path::Path;

use crate::core::*;

use fjall::PartitionCreateOptions;
use kolme_store::{KolmeStoreError, StorableBlock};

use super::BlockHeight;

#[derive(Clone)]
pub struct KolmeStoreFjall {
    _keyspace: fjall::Keyspace,
    handle: fjall::PartitionHandle,
}

impl KolmeStoreFjall {
    pub fn new(fjall_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let keyspace = fjall::Config::new(fjall_dir).open()?;
        let handle = keyspace.open_partition("kolme", PartitionCreateOptions::default())?;
        Ok(Self {
            _keyspace: keyspace,
            handle,
        })
    }

    pub fn load_latest_block(&self) -> Result<Option<BlockHeight>, KolmeStoreError> {
        let Some(latest) = self.handle.prefix("block:").next_back() else {
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
            .handle
            .get(block_key(height))
            .map_err(KolmeStoreError::custom)?
            .ok_or(KolmeStoreError::BlockNotFound { height: height.0 })?;
        let hash = Sha256Hash::from_hash(&hash_bytes).map_err(KolmeStoreError::custom)?;
        merkle_manager
            .load(&mut FjallMerkleStore(&self.handle), hash)
            .await
            .map_err(KolmeStoreError::custom)
    }

    pub async fn add_block<App: KolmeApp>(
        &self,
        merkle_manager: &MerkleManager,
        block: &StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>,
    ) -> Result<()> {
        let key = block_key(BlockHeight(block.height));
        if self.handle.contains_key(key)? {
            Err(KolmeStoreError::BlockAlreadyInDb {
                height: block.height,
            }
            .into())
        } else {
            let contents = merkle_manager
                .save(&mut FjallMerkleStore(&self.handle), block)
                .await?;

            // TODO do we worry about race conditions?
            self.handle.insert(key, contents.hash.as_array())?;
            self.handle
                .insert(tx_key(TxHash(block.txhash)), block.height.to_be_bytes())?;
            // TODO should we persist here too?
            Ok(())
        }
    }

    pub fn get_height_for_tx(&self, txhash: TxHash) -> Result<Option<BlockHeight>> {
        let Some(height) = self.handle.get(tx_key(txhash))? else {
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
            .handle
            .first_key_value()
            .map_err(KolmeStoreError::custom)?
        {
            self.handle.remove(key).map_err(KolmeStoreError::custom)?;
        }
        Ok(())
    }

    pub(super) fn get_merkle_store(&self) -> FjallMerkleStore {
        FjallMerkleStore(&self.handle)
    }
}

fn block_key(height: BlockHeight) -> [u8; 14] {
    let mut array = [b'b', b'l', b'o', b'c', b'k', b':', 0, 0, 0, 0, 0, 0, 0, 0];
    array[6..].copy_from_slice(&height.0.to_be_bytes());
    array
}

fn hash_key(hash: Sha256Hash) -> [u8; 37] {
    let mut array = [0; 37];
    array[0..5].copy_from_slice(b"hash:");
    array[5..].copy_from_slice(hash.as_array());
    array
}

fn tx_key(tx: TxHash) -> [u8; 35] {
    let mut array = [0; 35];
    array[0..3].copy_from_slice(b"tx:");
    array[3..].copy_from_slice(tx.0.as_array());
    array
}

pub(super) struct FjallMerkleStore<'a>(&'a fjall::PartitionHandle);

impl MerkleStore for FjallMerkleStore<'_> {
    async fn load_by_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<Arc<[u8]>>, MerkleSerialError> {
        self.0
            .get(hash_key(hash))
            .map(|oslice| oslice.map(|slice| Arc::<[u8]>::from(slice.to_vec())))
            .map_err(MerkleSerialError::custom)
    }

    async fn save_by_hash(
        &mut self,
        hash: Sha256Hash,
        payload: &[u8],
    ) -> Result<(), MerkleSerialError> {
        self.0
            .insert(hash_key(hash), payload)
            .map_err(MerkleSerialError::custom)
    }

    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        self.0
            .contains_key(hash_key(hash))
            .map_err(MerkleSerialError::custom)
    }
}
