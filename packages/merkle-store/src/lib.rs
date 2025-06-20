pub use fjall::*;
use merkle_map::{MerkleMemoryStore, MerkleSerialError, MerkleStore};
use merkle_store_fjall::MerkleFjallStore;
use merkle_store_postgres::{
    sqlx::{pool::PoolOptions, Postgres},
    MerklePostgresStore,
};
use std::path::Path;

mod fjall;

#[derive(Clone)]
pub enum KolmeMerkleStore {
    Memory(MerkleMemoryStore),
    Fjall(MerkleFjallStore),
    Postgres(MerklePostgresStore),
}
impl Default for KolmeMerkleStore {
    fn default() -> Self {
        Self::Memory(Default::default())
    }
}
impl MerkleStore for KolmeMerkleStore {
    async fn load_by_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
    ) -> Result<Option<merkle_map::MerkleLayerContents>, MerkleSerialError> {
        match self {
            KolmeMerkleStore::Fjall(store) => store.load_by_hash(hash).await,
            KolmeMerkleStore::Postgres(store) => store.load_by_hash(hash).await,
            KolmeMerkleStore::Memory(store) => store.load_by_hash(hash).await,
        }
    }

    async fn save_by_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
        layer: &merkle_map::MerkleLayerContents,
    ) -> Result<(), MerkleSerialError> {
        match self {
            KolmeMerkleStore::Fjall(store) => store.save_by_hash(hash, layer).await,
            KolmeMerkleStore::Postgres(store) => store.save_by_hash(hash, layer).await,
            KolmeMerkleStore::Memory(store) => store.save_by_hash(hash, layer).await,
        }
    }

    async fn contains_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
    ) -> Result<bool, MerkleSerialError> {
        match self {
            KolmeMerkleStore::Fjall(store) => store.contains_hash(hash).await,
            KolmeMerkleStore::Postgres(store) => store.contains_hash(hash).await,
            KolmeMerkleStore::Memory(store) => store.contains_hash(hash).await,
        }
    }
}
impl From<MerkleFjallStore> for KolmeMerkleStore {
    fn from(value: MerkleFjallStore) -> Self {
        KolmeMerkleStore::Fjall(value)
    }
}
impl From<MerklePostgresStore> for KolmeMerkleStore {
    fn from(value: MerklePostgresStore) -> Self {
        KolmeMerkleStore::Postgres(value)
    }
}
impl From<MerkleMemoryStore> for KolmeMerkleStore {
    fn from(value: MerkleMemoryStore) -> Self {
        KolmeMerkleStore::Memory(value)
    }
}
impl KolmeMerkleStore {
    pub fn new_fjall(db_path: impl AsRef<Path>) -> Result<Self, MerkleSerialError> {
        Ok(KolmeMerkleStore::Fjall(MerkleFjallStore::new(db_path)?))
    }

    pub async fn new_postgres_with_url(url: impl AsRef<str>) -> Result<Self, MerkleSerialError> {
        Ok(KolmeMerkleStore::Postgres(
            MerklePostgresStore::new(url).await?,
        ))
    }
    pub async fn new_postgres_with_pool_options(
        url: impl AsRef<str>,
        options: PoolOptions<Postgres>,
    ) -> Result<Self, MerkleSerialError> {
        Ok(KolmeMerkleStore::Postgres(
            MerklePostgresStore::new_with_options(url, options).await?,
        ))
    }

    pub fn fjall(&self) -> Option<&MerkleFjallStore> {
        match self {
            KolmeMerkleStore::Fjall(fjall) => Some(fjall),
            _ => None,
        }
    }
}

trait Sealed {}
impl Sealed for MerklePostgresStore {}
impl Sealed for MerkleFjallStore {}
impl Sealed for MerkleMemoryStore {}

#[allow(private_bounds)]
pub trait Not<T>: Sealed {}
impl Not<MerkleFjallStore> for MerklePostgresStore {}
impl Not<MerkleFjallStore> for MerkleMemoryStore {}
