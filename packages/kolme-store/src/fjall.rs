use crate::{
    r#trait::KolmeBackingStore, KolmeConstructLock, KolmeStoreError, StorableBlock,
    DEFAULT_CACHE_SIZE,
};
use anyhow::Context;
use lru::LruCache;
use merkle_map::{
    MerkleDeserializeRaw, MerkleLayerContents, MerkleSerializeRaw, MerkleStore as _, Sha256Hash,
};
use parking_lot::Mutex;
use std::{num::NonZeroUsize, path::Path, sync::Arc};

mod merkle;

const LATEST_ARCHIVED_HEIGHT_KEY: &[u8] = b"LATEST";

#[derive(Clone)]
pub struct Store {
    pub(super) merkle: merkle::MerkleFjallStore,
    cache: Arc<Mutex<LruCache<Sha256Hash, MerkleLayerContents>>>,
}

impl Store {
    pub fn new(fjall_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Store::new_with(fjall_dir, DEFAULT_CACHE_SIZE)
    }

    pub fn new_with(fjall_dir: impl AsRef<Path>, cache_size: usize) -> anyhow::Result<Self> {
        let merkle = merkle::MerkleFjallStore::new(fjall_dir)?;

        Ok(Self {
            merkle,
            cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(cache_size)
                    .context("Store::new_with: provided 0 as cache size")?,
            ))),
        })
    }
}

impl KolmeBackingStore for Store {
    async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
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

        self.cache.lock().clear();
        Ok(())
    }

    async fn delete_block(&self, _height: u64) -> Result<(), KolmeStoreError> {
        Err(KolmeStoreError::UnsupportedDeleteOperation("Fjall"))
    }

    async fn take_construct_lock(&self) -> Result<crate::KolmeConstructLock, KolmeStoreError> {
        Ok(KolmeConstructLock::NoLocking)
    }

    async fn get_merkle_layer(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<merkle_map::MerkleLayerContents>, merkle_map::MerkleSerialError> {
        let layer = self.cache.lock().get(&hash).cloned();
        if let Some(layer) = layer {
            Ok(Some(layer))
        } else {
            self.merkle.clone().load_by_hash(hash)
        }
    }

    async fn get_height_for_tx(&self, txhash: Sha256Hash) -> anyhow::Result<Option<u64>> {
        let Some(height) = self.merkle.handle.get(tx_key(txhash))? else {
            return Ok(None);
        };
        let height = match <[u8; 8]>::try_from(&*height) {
            Ok(height) => u64::from_be_bytes(height),
            Err(e) => anyhow::bail!("get_height_for_tx: invalid height in Fjall store: {e}"),
        };
        Ok(Some(height))
    }

    async fn load_latest_block(&self) -> Result<Option<u64>, KolmeStoreError> {
        let Some(latest) = self.merkle.handle.prefix("block:").next_back() else {
            return Ok(None);
        };
        let (key, _hash_bytes) = latest.map_err(KolmeStoreError::custom)?;
        let key = (*key)
            .strip_prefix(b"block:")
            .ok_or_else(|| KolmeStoreError::Other("Fjall key missing block: prefix".to_owned()))?;
        let height = <[u8; 8]>::try_from(key).map_err(KolmeStoreError::custom)?;
        Ok(Some(u64::from_be_bytes(height)))
    }

    async fn load_block<Block>(
        &self,
        height: u64,
    ) -> Result<Option<StorableBlock<Block>>, KolmeStoreError>
    where
        Block: MerkleDeserializeRaw + MerkleSerializeRaw,
    {
        let Some(hash_bytes) = self
            .merkle
            .handle
            .get(block_key(height))
            .map_err(KolmeStoreError::custom)?
        else {
            return Ok(None);
        };
        let hash = Sha256Hash::from_hash(&hash_bytes).map_err(KolmeStoreError::custom)?;
        let mut store = self.merkle.clone();
        merkle_map::load(&mut store, hash)
            .await
            .map_err(KolmeStoreError::custom)
            .map(Some)
    }

    async fn has_block(&self, height: u64) -> Result<bool, KolmeStoreError> {
        self.merkle
            .handle
            .contains_key(block_key(height))
            .map_err(KolmeStoreError::custom)
    }

    async fn has_merkle_hash(
        &self,
        hash: Sha256Hash,
    ) -> Result<bool, merkle_map::MerkleSerialError> {
        let cached = self.cache.lock().contains(&hash);
        if cached {
            return Ok(true);
        }
        let mut merkle = self.merkle.clone();
        merkle.contains_hash(hash).await
    }

    async fn add_block<Block>(&self, block: &StorableBlock<Block>) -> Result<(), KolmeStoreError>
    where
        Block: MerkleSerializeRaw,
    {
        let key = block_key(block.height);
        let contents = merkle_map::api::serialize(block).map_err(KolmeStoreError::custom)?;

        if let Some(existing_hash) = self
            .merkle
            .handle
            .get(key)
            .map_err(KolmeStoreError::custom)?
        {
            if existing_hash != contents.hash.as_array() {
                return Err(KolmeStoreError::ConflictingBlockInDb {
                    height: block.height,
                    existing: Sha256Hash::from_hash(&existing_hash)
                        .map_err(KolmeStoreError::custom)?,
                    adding: contents.hash,
                });
            } else {
                return Err(KolmeStoreError::MatchingBlockAlreadyInserted {
                    height: block.height,
                });
            }
        }

        let mut store = self.merkle.clone();
        let hash = contents.hash;
        merkle_map::api::save_merkle_contents(&mut store, contents).await?;

        self.merkle
            .handle
            .insert(key, hash.as_array())
            .map_err(KolmeStoreError::custom)?;
        self.merkle
            .handle
            .insert(tx_key(block.txhash), block.height.to_be_bytes())
            .map_err(KolmeStoreError::custom)?;
        self.merkle
            .keyspace
            .persist(fjall::PersistMode::SyncAll)
            .map_err(KolmeStoreError::custom)?;

        Ok(())
    }

    async fn add_merkle_layer(
        &self,
        hash: Sha256Hash,
        layer: &merkle_map::MerkleLayerContents,
    ) -> anyhow::Result<()> {
        let mut merkle = self.merkle.clone();
        merkle.save_by_hash(hash, layer).await?;
        self.cache.lock().put(hash, layer.clone());
        Ok(())
    }

    async fn save<T>(&self, value: &T) -> anyhow::Result<std::sync::Arc<merkle_map::MerkleContents>>
    where
        T: merkle_map::MerkleSerializeRaw,
    {
        let mut store = self.merkle.clone();
        let contents = merkle_map::save(&mut store, value).await?;
        Ok(contents)
    }

    async fn load<T>(&self, hash: Sha256Hash) -> Result<T, merkle_map::MerkleSerialError>
    where
        T: merkle_map::MerkleDeserializeRaw,
    {
        let mut store = self.merkle.clone();
        merkle_map::load(&mut store, hash).await
    }

    async fn archive_block(&self, height: u64) -> anyhow::Result<()> {
        self.merkle
            .handle
            .insert(LATEST_ARCHIVED_HEIGHT_KEY, height.to_be_bytes())
            .context("Unable to update partition with given height")?;

        Ok(())
    }

    async fn get_latest_archived_block_height(&self) -> anyhow::Result<Option<u64>> {
        Ok(self
            .merkle
            .handle
            .get(LATEST_ARCHIVED_HEIGHT_KEY)
            .context("Unable to retrieve latest height")?
            .map(|contents| u64::from_be_bytes(std::array::from_fn(|i| contents[i]))))
    }
}

fn block_key(height: u64) -> [u8; 14] {
    let mut array = [b'b', b'l', b'o', b'c', b'k', b':', 0, 0, 0, 0, 0, 0, 0, 0];
    array[6..].copy_from_slice(&height.to_be_bytes());
    array
}

fn tx_key(tx: Sha256Hash) -> [u8; 35] {
    let mut array = [0; 35];
    array[0..3].copy_from_slice(b"tx:");
    array[3..].copy_from_slice(tx.as_array());
    array
}
