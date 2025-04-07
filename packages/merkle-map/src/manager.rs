use std::sync::LazyLock;

use dashmap::DashMap;
use shared::types::Sha256Hash;

use crate::*;

pub struct MerkleManager<Store> {
    pub(crate) store: Arc<Store>,
    pub(crate) cache: Arc<DashMap<Sha256Hash, Arc<[u8]>>>,
}

impl<Store> Clone for MerkleManager<Store> {
    fn clone(&self) -> Self {
        MerkleManager {
            store: self.store.clone(),
            cache: self.cache.clone(),
        }
    }
}

impl<Store> MerkleManager<Store> {
    /// Create a new manager based on the given storage engine.
    pub fn new(store: Store) -> Self {
        MerkleManager {
            store: Arc::new(store),
            cache: Arc::new(DashMap::new()),
        }
    }

    /// Create a new serializer that delegates storing blobs to this manager.
    pub fn new_serializer(&self) -> MerkleSerializerImpl<Store> {
        MerkleSerializerImpl {
            buff: Vec::new(),
            manager: self.clone(),
        }
    }

    /// Create a new deserializer that loads data from this manager.
    pub fn new_deserializer<'a>(&self, buff: &'a [u8]) -> MerkleDeserializerImpl<'a, Store> {
        MerkleDeserializerImpl {
            buff,
            pos: 0,
            manager: self.clone(),
        }
    }
}

impl<Store: MerkleStore> MerkleManager<Store> {
    pub(crate) fn save_merkle_by_hash(
        &self,
        hash: Sha256Hash,
        payload: Arc<[u8]>,
    ) -> Result<(), MerkleSerialError> {
        if !self.store.contains_hash(hash)? {
            self.store.save_merkle_by_hash(hash, &payload)?;
        }
        if !self.cache.contains_key(&hash) {
            self.cache.insert(hash, payload);
        }
        Ok(())
    }
}

pub(crate) fn empty() -> (Sha256Hash, Arc<[u8]>) {
    static EMPTY: LazyLock<(Sha256Hash, Arc<[u8]>)> = LazyLock::new(|| {
        let payload = vec![41].into();
        let hash = Sha256Hash::hash(&payload);
        (hash, payload)
    });
    EMPTY.clone()
}

impl<Store: MerkleStore> MerkleManager<Store> {
    pub fn save<T: MerkleSerializeComplete>(
        &self,
        x: &mut T,
    ) -> Result<Sha256Hash, MerkleSerialError> {
        x.serialize_complete(self)
    }
}

impl<Store: MerkleStore> MerkleManager<Store> {
    pub fn load<K: FromMerkleKey, V: MerkleDeserialize>(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleMap<K, V>>, MerkleSerialError> {
        let payload = match self.cache.get(&hash) {
            Some(payload) => Some(payload.value().clone()),
            None => {
                let payload = self.store.load_merkle_by_hash(hash)?;
                if let Some(payload) = payload.clone() {
                    self.cache.insert(hash, payload);
                }
                payload
            }
        };
        match payload {
            None => Ok(None),
            Some(payload) => {
                let node = Node::load(hash, payload, self)?;
                Ok(Some(MerkleMap(node)))
            }
        }
    }
}
