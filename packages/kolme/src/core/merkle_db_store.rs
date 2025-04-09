use merkle_map::{MerkleSerialError, MerkleStore};

pub(crate) struct MerkleDbStore<'a>(pub(crate) &'a sqlx::SqlitePool);

impl<'a> MerkleStore for MerkleDbStore<'a> {
    async fn load_by_hash(
        &mut self,
        hash: shared::types::Sha256Hash,
    ) -> Result<Option<std::sync::Arc<[u8]>>, merkle_map::MerkleSerialError> {
        sqlx::query_scalar!("SELECT payload FROM merkle_hashes WHERE hash=$1", hash)
            .fetch_optional(self.0)
            .await
            .map(|x| x.map(Into::into))
            .map_err(MerkleSerialError::custom)
    }

    async fn save_by_hash(
        &mut self,
        hash: shared::types::Sha256Hash,
        payload: &[u8],
    ) -> Result<(), merkle_map::MerkleSerialError> {
        sqlx::query!(
            "INSERT OR IGNORE INTO merkle_hashes(hash, payload) VALUES($1, $2)",
            hash,
            payload
        )
        .execute(self.0)
        .await
        .map_err(MerkleSerialError::custom)?;
        Ok(())
    }

    async fn contains_hash(
        &mut self,
        hash: shared::types::Sha256Hash,
    ) -> Result<bool, merkle_map::MerkleSerialError> {
        sqlx::query_scalar!("SELECT COUNT(*) FROM merkle_hashes WHERE hash=$1", hash)
            .fetch_one(self.0)
            .await
            .map(|x| {
                assert!(x == 0 || x == 1);
                x == 1
            })
            .map_err(MerkleSerialError::custom)
    }
}
