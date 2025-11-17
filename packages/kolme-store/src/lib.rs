mod block;
mod error;
pub mod fjall;
mod in_memory;
pub mod postgres;
mod r#trait;

pub use block::{BlockHashes, StorableBlock};
pub use error::KolmeStoreError;
use fjall::Store as KolmeFjallStore;
use in_memory::Store as KolmeInMemoryStore;
use merkle_map::{
    MerkleDeserializeRaw, MerkleLayerContents, MerkleSerialError, MerkleSerializeRaw, Sha256Hash,
};
use postgres::Store as KolmePostgresStore;
pub use r#trait::{HasBlockHashes, KolmeBackingStore};
pub use sqlx;
use sqlx::{pool::PoolOptions, postgres::PgConnectOptions, Postgres};
use std::{path::Path, sync::Arc};
use tokio::sync::OwnedSemaphorePermit;

pub enum KolmeConstructLock {
    NoLocking,
    Postgres { _lock: postgres::ConstructLock },
    InProcess { _lock: OwnedSemaphorePermit },
}

#[enum_dispatch::enum_dispatch]
#[derive(Clone)]
pub enum KolmeStore {
    KolmeInMemoryStore,
    KolmeFjallStore,
    KolmePostgresStore,
}

impl KolmeStore {
    pub async fn new_postgres(url: &str) -> anyhow::Result<Self> {
        Ok(KolmeStore::KolmePostgresStore(
            postgres::Store::new(url).await?,
        ))
    }
    pub async fn new_postgres_with_options(
        connect: PgConnectOptions,
        options: PoolOptions<Postgres>,
    ) -> anyhow::Result<Self> {
        Ok(KolmeStore::KolmePostgresStore(
            postgres::Store::new_with_options(connect, options).await?,
        ))
    }

    pub fn new_fjall(fjall_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(KolmeStore::KolmeFjallStore(fjall::Store::new(fjall_dir)?))
    }

    pub fn new_in_memory() -> Self {
        KolmeStore::KolmeInMemoryStore(in_memory::Store::default())
    }

    pub async fn load_signed_block<Block, FrameworkState, AppState>(
        &self,
        height: u64,
    ) -> Result<Option<Arc<Block>>, KolmeStoreError>
    where
        Block: serde::de::DeserializeOwned + MerkleDeserializeRaw + MerkleSerializeRaw,
        FrameworkState: MerkleDeserializeRaw + MerkleSerializeRaw,
        AppState: MerkleDeserializeRaw + MerkleSerializeRaw,
    {
        Ok(match &self {
            KolmeStore::KolmePostgresStore(kolme_store_postgres) => kolme_store_postgres
                .load_rendered_block(height)
                .await?
                .map(|s| {
                    serde_json::from_str(&s)
                        .map_err(KolmeStoreError::custom)
                        .map(Arc::new)
                })
                .transpose()
                .map_err(KolmeStoreError::custom)?,
            KolmeStore::KolmeFjallStore(kolme_store_fjall) => kolme_store_fjall
                .load_block::<Block>(height)
                .await
                .map_err(KolmeStoreError::custom)?
                .map(|x| x.block),
            KolmeStore::KolmeInMemoryStore(kolme_store_in_memory) => kolme_store_in_memory
                .load_block::<Block>(height)
                .await
                .map_err(KolmeStoreError::custom)?
                .map(|x| x.block),
        })
    }
}
