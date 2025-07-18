//! Helper types and functions for buffer reading and writing.

use std::{collections::HashSet, num::NonZeroUsize, sync::Arc};

use lru::LruCache;
use parking_lot::Mutex;
use shared::types::Sha256Hash;
use smallvec::SmallVec;

use crate::*;

/// Primary interface for loading and saving data with merkle-map.
#[derive(Clone)]
pub struct MerkleManager {
    //cache: Arc<parking_lot::RwLock<HashMap<Sha256Hash, Arc<[u8]>>>>,
    cache: Arc<Mutex<LruCache<Sha256Hash, Arc<[u8]>>>>,
}

impl MerkleSerialError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(Box::new(e))
    }
}

impl std::fmt::Debug for MerkleContents {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "MerkleContents({})", self.hash)
    }
}

impl MerkleManager {
    pub fn new(cache_size: usize) -> Self {
        let cache = LruCache::new(NonZeroUsize::new(cache_size).unwrap());
        Self {
            cache: Arc::new(Mutex::new(cache)),
        }
    }

    /// Serialize a value into a [MerkleContents] for later storage.
    pub fn serialize<T: MerkleSerializeRaw + ?Sized>(
        &self,
        value: &T,
    ) -> Result<Arc<MerkleContents>, MerkleSerialError> {
        if let Some(contents) = value.get_merkle_contents_raw() {
            return Ok(contents);
        }

        let mut serializer = MerkleSerializer::new(self.clone());
        value.merkle_serialize_raw(&mut serializer)?;
        let mut contents = serializer.finish();
        let existing_payload = self.cache.lock().get(&contents.hash).cloned();
        match existing_payload {
            Some(payload) => {
                // Reuse existing memory
                contents.payload = payload.clone();
            }
            None => {
                let payload = contents.payload.clone();
                self.cache.lock().put(contents.hash, payload);
            }
        }

        let contents = Arc::new(contents);
        value.set_merkle_contents_raw(&contents);
        Ok(contents)
    }

    /// Save a [MerkleContents] to the given store.
    pub async fn save_merkle_contents<Store: MerkleStore>(
        &self,
        store: &mut Store,
        contents: &MerkleContents,
    ) -> Result<(), MerkleSerialError> {
        // First check if our hash already exists. If so, we assume
        // all children are also present.
        if store.contains_hash(contents.hash).await? {
            return Ok(());
        }

        // If the hash doesn't exist, write the children first.
        // This is to ensure that all child data is present before
        // writing the parent. This is what allows us to have the short-circuit
        // optimization above.
        let mut children = SmallVec::with_capacity(contents.children.len());
        for child in contents.children.iter() {
            children.push(child.hash);
            let future = Box::pin(self.save_merkle_contents(store, child));
            future.await?;
        }

        // And finally write the actual contents.
        let layer = MerkleLayerContents {
            payload: contents.payload.clone(),
            children,
        };
        store.save_by_hash(contents.hash, &layer).await?;

        Ok(())
    }

    /// Serialize a save a value.
    pub async fn save<T: MerkleSerializeRaw, Store: MerkleStore>(
        &self,
        store: &mut Store,
        value: &T,
    ) -> Result<Arc<MerkleContents>, MerkleSerialError> {
        let contents = self.serialize(value)?;
        self.save_merkle_contents(store, &contents).await?;
        Ok(contents)
    }

    /// Deserialize a value from the given payload.
    pub fn deserialize<T: MerkleDeserializeRaw>(
        &self,
        hash: Sha256Hash,
        payload: Arc<[u8]>,
    ) -> Result<T, MerkleSerialError> {
        let mut deserializer = MerkleDeserializer::new(hash, payload, self.clone());
        let value = T::merkle_deserialize_raw(&mut deserializer)?;
        let contents = Arc::new(deserializer.finish()?);
        value.set_merkle_contents_raw(&contents);
        Ok(value)
    }

    /// Load the value at the given hash
    pub async fn load<T: MerkleDeserializeRaw, Store: MerkleStore>(
        &self,
        store: &mut Store,
        hash: Sha256Hash,
    ) -> Result<T, MerkleSerialError> {
        // We load the data in a loop. Each time we encounter an
        // error about missing hashes, we load up the missing data and try again.
        let payload = self.get_or_load_payload(store, hash).await?;
        loop {
            match self.deserialize(hash, payload.clone()) {
                Ok(value) => break Ok(value),
                Err(MerkleSerialError::HashesNotFound { hashes }) => {
                    for hash in hashes {
                        self.get_or_load_payload(store, hash).await?;
                    }
                }
                Err(e) => break Err(e),
            }
        }
    }

    pub(crate) async fn get_or_load_payload<Store: MerkleStore>(
        &self,
        store: &mut Store,
        hash: Sha256Hash,
    ) -> Result<Arc<[u8]>, MerkleSerialError> {
        if let Some(payload) = self.cache.lock().get(&hash) {
            return Ok(payload.clone());
        }

        let MerkleLayerContents { payload, children } = store
            .load_by_hash(hash)
            .await?
            .ok_or_else(|| MerkleSerialError::HashesNotFound {
                hashes: HashSet::from_iter([hash]),
            })?;
        for child in children {
            let future = Box::pin(self.get_or_load_payload(store, child));
            future.await?;
        }
        self.cache.lock().put(hash, payload.clone());

        Ok(payload)
    }

    pub(crate) fn deserialize_cached<T: MerkleDeserializeRaw>(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<T>, MerkleSerialError> {
        let Some(payload) = self.cache.lock().get(&hash).cloned() else {
            return Ok(None);
        };
        self.deserialize(hash, payload).map(Some)
    }
}
