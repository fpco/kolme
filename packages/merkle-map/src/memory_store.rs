use std::{collections::HashMap, sync::RwLock};

use shared::types::Sha256Hash;

use crate::*;

#[derive(Clone, Default)]
pub struct MerkleMemoryStore(Arc<RwLock<HashMap<Sha256Hash, MerkleLayerContents>>>);

impl MerkleMemoryStore {
    pub fn get_map_snapshot(&self) -> HashMap<Sha256Hash, MerkleLayerContents> {
        self.0.read().unwrap().clone()
    }
}

impl MerkleStore for MerkleMemoryStore {
    async fn load_by_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleLayerContents>, MerkleSerialError> {
        Ok(self.0.read().unwrap().get(&hash).cloned())
    }

    async fn save_by_hash(
        &mut self,
        hash: Sha256Hash,
        payload: &MerkleLayerContents,
    ) -> Result<(), MerkleSerialError> {
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
