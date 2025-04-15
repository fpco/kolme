use merkle_map::{MerkleSerialError, MerkleStore};

pub(crate) enum MerkleDbStore<'a> {
    Conn(&'a mut sqlx::SqliteConnection),
    Pool(&'a sqlx::SqlitePool),
}

impl<'a> MerkleStore for MerkleDbStore<'a> {
    async fn load_by_hash(
        &mut self,
        hash: shared::types::Sha256Hash,
    ) -> Result<Option<std::sync::Arc<[u8]>>, merkle_map::MerkleSerialError> {
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
        let query = sqlx::query!(
            "INSERT OR IGNORE INTO merkle_hashes(hash, payload) VALUES($1, $2)",
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
        let query = sqlx::query_scalar!("SELECT COUNT(*) FROM merkle_hashes WHERE hash=$1", hash);
        let res = match self {
            MerkleDbStore::Conn(conn) => query.fetch_one(&mut **conn).await,
            MerkleDbStore::Pool(pool) => query.fetch_one(*pool).await,
        };
        res.map(|x| {
            assert!(x == 0 || x == 1);
            x == 1
        })
        .map_err(MerkleSerialError::custom)
    }
}
