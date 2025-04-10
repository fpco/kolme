//! Helper types and functions for buffer reading and writing.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use shared::types::Sha256Hash;

use crate::*;

/// Primary interface for loading and saving data with merkle-map.
#[derive(Clone, Default)]
pub struct MerkleManager {
    cache: Arc<parking_lot::RwLock<HashMap<Sha256Hash, Arc<[u8]>>>>,
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
    /// Serialize a value into a [MerkleContents] for later storage.
    pub fn serialize<T: MerkleSerialize>(
        &self,
        value: &T,
    ) -> Result<Arc<MerkleContents>, MerkleSerialError> {
        if let Some(contents) = value.get_merkle_contents() {
            return Ok(contents);
        }

        let mut serializer = MerkleSerializer::new(self.clone());
        value.merkle_serialize(&mut serializer)?;
        let contents = Arc::new(serializer.finish());

        if !self.cache.read().contains_key(&contents.hash) {
            self.cache
                .write()
                .insert(contents.hash, contents.payload.clone());
        }
        value.set_merkle_contents(contents.clone());
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
        for child in contents.children.iter() {
            let future = Box::pin(self.save_merkle_contents(store, child));
            future.await?;
        }

        // And finally write the actual contents.
        store.save_by_hash(contents.hash, &contents.payload).await?;

        Ok(())
    }

    /// Serialize a save a value.
    pub async fn save<T: MerkleSerialize, Store: MerkleStore>(
        &self,
        store: &mut Store,
        value: &T,
    ) -> Result<Arc<MerkleContents>, MerkleSerialError> {
        let contents = self.serialize(value)?;
        self.save_merkle_contents(store, &contents).await?;
        Ok(contents)
    }

    /// Deserialize a value from the given payload.
    pub fn deserialize<T: MerkleDeserialize>(
        &self,
        hash: Sha256Hash,
        payload: Arc<[u8]>,
    ) -> Result<T, MerkleSerialError> {
        let mut deserializer = MerkleDeserializer::new(hash, payload, self.clone());
        let value = T::merkle_deserialize(&mut deserializer)?;
        let contents = Arc::new(deserializer.finish()?);
        value.set_merkle_contents(contents);
        Ok(value)
    }

    /// Load the value at the given hash
    pub async fn load<T: MerkleDeserialize, Store: MerkleStore>(
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
        if let Some(payload) = self.cache.read().get(&hash) {
            return Ok(payload.clone());
        }

        let payload =
            store
                .load_by_hash(hash)
                .await?
                .ok_or_else(|| MerkleSerialError::HashesNotFound {
                    hashes: HashSet::from_iter([hash]),
                })?;
        self.cache.write().insert(hash, payload.clone());

        Ok(payload)
    }

    pub(crate) fn deserialize_cached<T: MerkleDeserialize>(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<T>, MerkleSerialError> {
        match self.cache.read().get(&hash) {
            None => Ok(None),
            Some(payload) => self.deserialize(hash, payload.clone()).map(Some),
        }
    }
}
