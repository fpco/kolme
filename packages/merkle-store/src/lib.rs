pub use fjall::*;
use merkle_map::{MerkleMemoryStore, MerkleSerialError, MerkleStore};
use merkle_store_cassandra::{
    MerkleCassandraStore, scylla::client::session_builder::SessionBuilder,
};
use merkle_store_fjall::MerkleFjallStore;
use std::path::Path;

mod fjall;

#[derive(Clone)]
pub enum KolmeMerkleStore {
    Memory(MerkleMemoryStore),
    Fjall(MerkleFjallStore),
    Cassandra(MerkleCassandraStore),
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
            KolmeMerkleStore::Cassandra(store) => store.load_by_hash(hash).await,
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
            KolmeMerkleStore::Cassandra(store) => store.save_by_hash(hash, layer).await,
            KolmeMerkleStore::Memory(store) => store.save_by_hash(hash, layer).await,
        }
    }

    async fn contains_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
    ) -> Result<bool, MerkleSerialError> {
        match self {
            KolmeMerkleStore::Fjall(store) => store.contains_hash(hash).await,
            KolmeMerkleStore::Cassandra(store) => store.contains_hash(hash).await,
            KolmeMerkleStore::Memory(store) => store.contains_hash(hash).await,
        }
    }
}
impl From<MerkleFjallStore> for KolmeMerkleStore {
    fn from(value: MerkleFjallStore) -> Self {
        KolmeMerkleStore::Fjall(value)
    }
}
impl From<MerkleCassandraStore> for KolmeMerkleStore {
    fn from(value: MerkleCassandraStore) -> Self {
        KolmeMerkleStore::Cassandra(value)
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
    pub async fn new_cassandra_with_known_nodes<NodeAddr: AsRef<str>>(
        known_nodes: impl IntoIterator<Item = NodeAddr>,
    ) -> Result<Self, MerkleSerialError> {
        Ok(KolmeMerkleStore::Cassandra(
            MerkleCassandraStore::new_with_known_nodes(known_nodes).await?,
        ))
    }
    pub async fn new_cassandra_with_builder(
        builder: SessionBuilder,
    ) -> Result<Self, MerkleSerialError> {
        Ok(KolmeMerkleStore::Cassandra(
            MerkleCassandraStore::new_with_builder(builder).await?,
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
impl Sealed for MerkleCassandraStore {}
impl Sealed for MerkleFjallStore {}
impl Sealed for MerkleMemoryStore {}

#[allow(private_bounds)]
pub trait Not<T>: Sealed {}
impl Not<MerkleFjallStore> for MerkleCassandraStore {}
impl Not<MerkleFjallStore> for MerkleMemoryStore {}
