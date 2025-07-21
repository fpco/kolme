//! Helper types and functions for buffer reading and writing.

use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    sync::{Arc, LazyLock},
};

use lru::LruCache;
use parking_lot::Mutex;
use shared::types::Sha256Hash;
use smallvec::SmallVec;

use crate::*;

/// Primary interface for loading and saving data with merkle-map.
#[derive(Clone)]
pub struct MerkleManager {
    //cache: Arc<parking_lot::RwLock<HashMap<Sha256Hash, Arc<[u8]>>>>,
    cache: Arc<Mutex<LruCache<Sha256Hash, MerkleLayerContents>>>,
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
    /// cache_size must be at least 1
    pub fn new(cache_size: usize) -> Self {
        let cache =
            LruCache::new(NonZeroUsize::new(cache_size).expect("Cannot have a cache size of 0"));
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
            Some(layer) => {
                // Reuse existing memory
                contents.payload = layer.payload.clone();
            }
            None => {
                self.cache.lock().put(
                    contents.hash,
                    MerkleLayerContents {
                        payload: contents.payload.clone(),
                        children: contents.children.iter().map(|m| m.hash).collect(),
                    },
                );
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
    ///
    /// This relies on all layers fitting in the LRU cache of the manager. This is finicky;
    /// consider moving to other methods.
    pub fn deserialize<T: MerkleDeserializeRaw>(
        &self,
        hash: Sha256Hash,
        payload: Arc<[u8]>,
    ) -> Result<T, MerkleSerialError> {
        static EMPTY_HASHMAP: LazyLock<Arc<HashMap<Sha256Hash, MerkleLayerContents>>> =
            LazyLock::new(|| Arc::new(HashMap::new()));
        self.deserialize_with(hash, payload, EMPTY_HASHMAP.clone())
    }

    fn deserialize_with<T: MerkleDeserializeRaw>(
        &self,
        hash: Sha256Hash,
        payload: Arc<[u8]>,
        loaded: Arc<HashMap<Sha256Hash, MerkleLayerContents>>,
    ) -> Result<T, MerkleSerialError> {
        let mut deserializer = MerkleDeserializer::new(hash, payload, self.clone(), loaded);
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
        let (layer, layers) = self.get_or_load_payload(store, hash).await?;
        self.deserialize_with(hash, layer.payload, Arc::new(layers))
    }

    pub(crate) async fn get_or_load_payload<Store: MerkleStore>(
        &self,
        store: &mut Store,
        hash: Sha256Hash,
    ) -> Result<
        (
            MerkleLayerContents,
            HashMap<Sha256Hash, MerkleLayerContents>,
        ),
        MerkleSerialError,
    > {
        let mut layers = HashMap::new();
        let mut to_process = vec![hash];

        loop {
            let Some(hash) = to_process.pop() else { break };

            let layer = self.cache.lock().get(&hash).cloned();
            let layer = if let Some(layer) = layer {
                layer
            } else {
                let layer = store.load_by_hash(hash).await?.ok_or_else(|| {
                    MerkleSerialError::HashesNotFound {
                        hashes: HashSet::from_iter([hash]),
                    }
                })?;
                self.cache.lock().put(hash, layer.clone());
                layer
            };

            to_process.extend_from_slice(&layer.children);
            layers.insert(hash, layer);
        }

        Ok((
            layers.get(&hash).expect("Impossible missing hash").clone(),
            layers,
        ))
    }

    pub(crate) fn deserialize_cached<T: MerkleDeserializeRaw>(
        &self,
        hash: Sha256Hash,
        loaded: Arc<HashMap<Sha256Hash, MerkleLayerContents>>,
    ) -> Result<Option<T>, MerkleSerialError> {
        let layer = if let Some(layer) = self.cache.lock().get(&hash) {
            layer.clone()
        } else if let Some(layer) = loaded.get(&hash) {
            layer.clone()
        } else {
            return Ok(None);
        };
        self.deserialize_with(hash, layer.payload, loaded).map(Some)
    }
}
