use dashmap::DashMap;
use shared::types::Sha256Hash;

use crate::*;

#[derive(Clone, Default)]
pub struct MerkleMemoryStore(Arc<DashMap<Sha256Hash, Arc<[u8]>>>);

impl MerkleStore for MerkleMemoryStore {
    async fn load_merkle_by_hash(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<Arc<[u8]>>, MerkleSerialError> {
        Ok(self.0.get(&hash).map(|x| x.value().clone()))
    }

    async fn save_merkle_by_hash(
        &self,
        hash: Sha256Hash,
        payload: &[u8],
    ) -> Result<(), MerkleSerialError> {
        if let Some(value) = self.0.get(&hash) {
            assert_eq!(&**value, payload);
            return Ok(());
        }
        self.0.insert(hash, payload.into());
        Ok(())
    }

    async fn contains_hash(&self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        Ok(self.0.contains_key(&hash))
    }
}
