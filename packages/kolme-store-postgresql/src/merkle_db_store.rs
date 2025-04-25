use merkle_map::{MerkleSerialError, MerkleStore};

pub(crate) enum MerkleDbStore<'a> {
    Conn(&'a mut sqlx::PgConnection),
    Pool(&'a sqlx::PgPool),
}

impl MerkleStore for MerkleDbStore<'_> {
    async fn load_by_hash(
        &mut self,
        hash: shared::types::Sha256Hash,
    ) -> Result<Option<std::sync::Arc<[u8]>>, merkle_map::MerkleSerialError> {
        let hash = hash.as_array().as_slice();
        let query = sqlx::query_scalar!("SELECT payload FROM merkle_hashes WHERE hash=$1", hash);
        let res = match self {
            MerkleDbStore::Conn(conn) => query.fetch_optional(&mut **conn).await,
            MerkleDbStore::Pool(pool) => query.fetch_optional(*pool).await,
        };
        res.map(|x| x.map(Into::into))
            .map_err(MerkleSerialError::custom)
    }

    async fn save_by_hash(
        &mut self,
        hash: shared::types::Sha256Hash,
        payload: &[u8],
    ) -> Result<(), merkle_map::MerkleSerialError> {
        let hash = hash.as_array().as_slice();
        let query = sqlx::query!(
            "INSERT INTO merkle_hashes(hash, payload) VALUES($1, $2) ON CONFLICT DO NOTHING",
            hash,
            payload
        );
        let res = match self {
            MerkleDbStore::Conn(conn) => query.execute(&mut **conn).await,
            MerkleDbStore::Pool(pool) => query.execute(*pool).await,
        };
        res.map_err(MerkleSerialError::custom)?;
        Ok(())
    }

    async fn contains_hash(
        &mut self,
        hash: shared::types::Sha256Hash,
    ) -> Result<bool, merkle_map::MerkleSerialError> {
        let hash = hash.as_array().as_slice();
        let query = sqlx::query_scalar!("SELECT COUNT(*) FROM merkle_hashes WHERE hash=$1", hash);
        let res = match self {
            MerkleDbStore::Conn(conn) => query.fetch_one(&mut **conn).await,
            MerkleDbStore::Pool(pool) => query.fetch_one(*pool).await,
        };
        res.map(|x| {
            let x = x.unwrap_or_default();
            assert!(x == 0 || x == 1);
            x == 1
        })
        .map_err(MerkleSerialError::custom)
    }
}
