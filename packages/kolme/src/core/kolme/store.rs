mod fjall;
mod in_memory;

use std::{collections::HashMap, path::Path};

use crate::core::*;

use fjall::KolmeStoreFjall;
use in_memory::KolmeStoreInMemory;
use kolme_store::{KolmeStoreError, StorableBlock};
use kolme_store_postgresql::{ConstructLock, KolmeStorePostgres};
use kolme_store_sqlite::KolmeStoreSqlite;
use parking_lot::RwLock;
use tokio::sync::OwnedSemaphorePermit;

#[derive(Clone)]
pub struct KolmeStore<App: KolmeApp> {
    inner: KolmeStoreInner,
    block_cache: Arc<RwLock<BlockCacheMap<App>>>,
}

#[allow(type_alias_bounds)]
type BlockCacheMap<App: KolmeApp> =
    HashMap<BlockHeight, StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>;

#[derive(Clone)]
enum KolmeStoreInner {
    Sqlite(KolmeStoreSqlite),
    Fjall(KolmeStoreFjall),
    Postgres(KolmeStorePostgres),
    InMemory(in_memory::KolmeStoreInMemory),
}

impl<App: KolmeApp> From<KolmeStoreInner> for KolmeStore<App> {
    fn from(inner: KolmeStoreInner) -> Self {
        Self {
            inner,
            block_cache: Default::default(),
        }
    }
}

pub enum KolmeConstructLock {
    NoLocking,
    Postgres { _lock: ConstructLock },
    InMemory { _permit: OwnedSemaphorePermit },
}

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn new_sqlite(db_path: impl AsRef<Path>) -> Result<Self> {
        KolmeStoreSqlite::new(db_path)
            .await
            .map(|x| KolmeStoreInner::Sqlite(x).into())
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres(url: &str) -> Result<Self> {
        KolmeStorePostgres::new(url)
            .await
            .map(|x| KolmeStoreInner::Postgres(x).into())
            .map_err(anyhow::Error::from)
    }

    pub fn new_fjall(dir: impl AsRef<Path>) -> Result<Self> {
        KolmeStoreFjall::new(dir).map(|x| KolmeStoreInner::Fjall(x).into())
    }

    pub fn new_in_memory() -> Self {
        KolmeStoreInner::InMemory(KolmeStoreInMemory::default()).into()
    }

    /// Ensures that either we have no blocks yet, or the first block has matching genesis info.
    pub(super) async fn validate_genesis_info(
        &self,
        merkle_manager: &MerkleManager,
        expected: &GenesisInfo,
    ) -> Result<()> {
        if let Some(actual) = self.load_genesis_info(merkle_manager).await? {
            anyhow::ensure!(
                &actual == expected,
                "Mismatched genesis info.\nActual:   {actual:?}\nExpected: {expected:?}"
            );
        }
        Ok(())
    }

    async fn load_genesis_info(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<GenesisInfo>> {
        let Some(block) = self
            .load_block(merkle_manager, BlockHeight::start())
            .await?
        else {
            return Ok(None);
        };
        let messages = &block.block.tx().0.message.as_inner().messages;
        anyhow::ensure!(messages.len() == 1);
        match messages.first().unwrap() {
            Message::Genesis(genesis_info) => Ok(Some(genesis_info.clone())),
            _ => Err(anyhow::anyhow!("Invalid messages in first block")),
        }
    }

    /// Very dangerous function! Intended for testing. Deletes all blocks in the database.
    pub async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        self.block_cache.write().clear();
        match &self.inner {
            KolmeStoreInner::Sqlite(kolme_store_sqlite) => kolme_store_sqlite.clear_blocks().await,
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                kolme_store_postgres.clear_blocks().await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory.clear_blocks().await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => kolme_store_fjall.clear_blocks(),
        }
    }

    pub(crate) async fn take_construct_lock(&self) -> Result<KolmeConstructLock> {
        match &self.inner {
            // No locking for SQLite
            KolmeStoreInner::Sqlite(_kolme_store_sqlite) => Ok(KolmeConstructLock::NoLocking),
            KolmeStoreInner::Postgres(kolme_store_postgres) => Ok(KolmeConstructLock::Postgres {
                _lock: kolme_store_postgres.take_construct_lock().await?,
            }),
            KolmeStoreInner::InMemory(kolme_store_in_memory) => Ok(KolmeConstructLock::InMemory {
                _permit: kolme_store_in_memory.take_construct_lock().await,
            }),
            KolmeStoreInner::Fjall(_kolme_store_fjall) => Ok(KolmeConstructLock::NoLocking),
        }
    }
}

impl<App: KolmeApp> KolmeStore<App> {
    pub async fn load_latest_block(&self) -> Result<Option<BlockHeight>> {
        Ok(match &self.inner {
            KolmeStoreInner::Sqlite(kolme_store_sqlite) => kolme_store_sqlite
                .load_latest_block()
                .await
                .map(|x| x.map(BlockHeight))?,
            KolmeStoreInner::Postgres(kolme_store_postgres) => kolme_store_postgres
                .load_latest_block()
                .await
                .map(|x| x.map(BlockHeight))?,
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory.load_latest_block().await?
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => kolme_store_fjall.load_latest_block()?,
        })
    }

    pub async fn load_block(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<Option<StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>> {
        if let Some(storable) = self.block_cache.read().get(&height) {
            return Ok(Some(storable.clone()));
        }
        let res = match &self.inner {
            KolmeStoreInner::Sqlite(kolme_store_sqlite) => {
                kolme_store_sqlite
                    .load_block(merkle_manager, height.0)
                    .await
            }
            KolmeStoreInner::Postgres(kolme_store_postgres) => {
                kolme_store_postgres
                    .load_block(merkle_manager, height.0)
                    .await
            }
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory
                    .load_block::<App>(merkle_manager, height)
                    .await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                kolme_store_fjall
                    .load_block::<App>(merkle_manager, height)
                    .await
            }
        };
        match res {
            Err(KolmeStoreError::BlockNotFound { height: _ }) => Ok(None),
            Err(e) => Err(e.into()),
            Ok(block) => Ok(Some(block)),
        }
    }

    pub(super) async fn get_height_for_tx(&self, txhash: TxHash) -> Result<Option<BlockHeight>> {
        match &self.inner {
            KolmeStoreInner::Sqlite(kolme_store_sqlite) => kolme_store_sqlite
                .get_height_for_tx(txhash.0)
                .await
                .map(|x| x.map(BlockHeight))
                .map_err(anyhow::Error::from),
            KolmeStoreInner::Postgres(kolme_store_postgres) => kolme_store_postgres
                .get_height_for_tx(txhash.0)
                .await
                .map(|x| x.map(BlockHeight))
                .map_err(anyhow::Error::from),
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory.get_height_for_tx(txhash).await
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                kolme_store_fjall.get_height_for_tx(txhash)
            }
        }
    }

    pub(super) async fn add_block(
        &self,
        merkle_manager: &MerkleManager,
        block: StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>,
    ) -> Result<()> {
        match &self.inner {
            KolmeStoreInner::Sqlite(kolme_store_sqlite) => kolme_store_sqlite
                .add_block(merkle_manager, &block)
                .await
                .map_err(anyhow::Error::from)?,
            KolmeStoreInner::Postgres(kolme_store_postgres) => kolme_store_postgres
                .add_block(merkle_manager, &block)
                .await
                .map_err(anyhow::Error::from)?,
            KolmeStoreInner::InMemory(kolme_store_in_memory) => {
                kolme_store_in_memory
                    .add_block::<App>(merkle_manager, &block)
                    .await?
            }
            KolmeStoreInner::Fjall(kolme_store_fjall) => {
                kolme_store_fjall
                    .add_block::<App>(merkle_manager, &block)
                    .await?
            }
        }
        let old = self
            .block_cache
            .write()
            .insert(BlockHeight(block.height), block);
        debug_assert!(old.is_none());
        Ok(())
    }
}
