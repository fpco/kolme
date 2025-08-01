use std::{collections::HashMap, sync::RwLock};

use shared::types::Sha256Hash;

use crate::*;

#[derive(Clone, Default)]
pub struct MerkleMemoryStore(Arc<RwLock<HashMap<Sha256Hash, MerkleLayerContents>>>);

impl MerkleMemoryStore {
    pub fn get_map_snapshot(&self) -> HashMap<Sha256Hash, MerkleLayerContents> {
        self.0.read().unwrap().clone()
    }

    pub fn load_by_hash(&self, hash: Sha256Hash) -> Option<MerkleLayerContents> {
        self.0.read().unwrap().get(&hash).cloned()
    }
}

impl MerkleStore for MerkleMemoryStore {
    async fn load_by_hashes(
        &mut self,
        hashes: &[Sha256Hash],
        dest: &mut HashMap<Sha256Hash, MerkleLayerContents>,
    ) -> Result<(), MerkleSerialError> {
        let guard = self.0.read().unwrap();
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
        if let Some(value) = self.0.read().unwrap().get(&hash) {
            assert_eq!(value, payload);
            return Ok(());
        }
        self.0.write().unwrap().insert(hash, payload.clone());
        Ok(())
    }

    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        Ok(self.0.read().unwrap().contains_key(&hash))
    }
}
