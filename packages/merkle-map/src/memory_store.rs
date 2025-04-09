use std::{collections::HashMap, sync::RwLock};

use shared::types::Sha256Hash;

use crate::*;

#[derive(Clone, Default)]
pub struct MerkleMemoryStore(Arc<RwLock<HashMap<Sha256Hash, Arc<[u8]>>>>);

impl MerkleStore for MerkleMemoryStore {
    async fn load_merkle_by_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<Arc<[u8]>>, MerkleSerialError> {
        Ok(self.0.read().unwrap().get(&hash).map(|x| x.clone()))
    }

    async fn save_merkle_by_hash(
        &mut self,
        hash: Sha256Hash,
        payload: &[u8],
    ) -> Result<(), MerkleSerialError> {
        if let Some(value) = self.0.read().unwrap().get(&hash) {
            assert_eq!(&**value, payload);
            return Ok(());
        }
        self.0.write().unwrap().insert(hash, payload.into());
        Ok(())
    }

    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        Ok(self.0.read().unwrap().contains_key(&hash))
    }
}
