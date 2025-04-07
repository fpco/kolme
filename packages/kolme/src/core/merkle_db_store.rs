use merkle_map::{MerkleSerialError, MerkleStore};

pub(crate) struct MerkleDbStore<'a>(pub(crate) &'a mut sqlx::SqliteTransaction<'a>);

impl<'a> MerkleStore for MerkleDbStore<'a> {
    async fn load_merkle_by_hash(
        &self,
        hash: shared::types::Sha256Hash,
    ) -> Result<Option<std::sync::Arc<[u8]>>, merkle_map::MerkleSerialError> {
        sqlx::query_scalar!("SELECT payload FROM merkle_hashes WHERE hash=$1", hash)
            .fetch_optional(&mut **self.0)
            .await
            .map(|x| x.map(Into::into))
            .map_err(MerkleSerialError::custom)
    }

    async fn save_merkle_by_hash(
        &self,
        hash: shared::types::Sha256Hash,
        payload: &[u8],
    ) -> Result<(), merkle_map::MerkleSerialError> {
        todo!()
    }

    async fn contains_hash(
        &self,
        hash: shared::types::Sha256Hash,
    ) -> Result<bool, merkle_map::MerkleSerialError> {
        todo!()
    }
}
