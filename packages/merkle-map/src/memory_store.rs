use std::{collections::HashMap, sync::RwLock};

use shared::types::Sha256Hash;

use crate::*;

#[derive(Clone, Default)]
pub struct MerkleMemoryStore {
    store: Arc<RwLock<HashMap<Sha256Hash, MerkleLayerContents>>>,
    no_dupes: bool,
}

impl MerkleMemoryStore {
    pub fn get_map_snapshot(&self) -> HashMap<Sha256Hash, MerkleLayerContents> {
        self.store.read().unwrap().clone()
    }

    pub fn load_by_hash(&self, hash: Sha256Hash) -> Option<MerkleLayerContents> {
        self.store.read().unwrap().get(&hash).cloned()
    }

    pub fn disallow_dupes(mut self) -> Self {
        self.no_dupes = true;
        self
    }
}

impl MerkleStore for MerkleMemoryStore {
    async fn load_by_hashes(
        &mut self,
        hashes: &[Sha256Hash],
        dest: &mut HashMap<Sha256Hash, MerkleLayerContents>,
    ) -> Result<(), MerkleSerialError> {
        let guard = self.store.read().unwrap();
        for hash in hashes {
            if let Some(layer) = guard.get(hash).cloned() {
                dest.insert(*hash, layer);
            }
        }
        Ok(())
    }

    async fn save_by_hash(
        &mut self,
        payload: &MerkleLayerContents,
    ) -> Result<(), MerkleSerialError> {
        let hash = payload.payload.hash();
        if let Some(value) = self.store.read().unwrap().get(&hash) {
            assert_eq!(value, payload);
            if self.no_dupes {
                panic!(
                    "Duplicate hash ({hash}) attempted to be saved in a no-dupe in-memory store"
                );
            }
            return Ok(());
        }
        self.store.write().unwrap().insert(hash, payload.clone());
        Ok(())
    }

    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        Ok(self.store.read().unwrap().contains_key(&hash))
    }
}
