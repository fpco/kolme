use std::path::Path;

use crate::core::*;

use kolme_store::{KolmeStoreError, StorableBlock};
use kolme_store_postgresql::KolmeStorePostgres;
use kolme_store_sqlite::KolmeStoreSqlite;

pub enum KolmeStore {
    Sqlite(KolmeStoreSqlite),
    Postgres(KolmeStorePostgres),
}

impl KolmeStore {
    pub async fn new_sqlite(db_path: impl AsRef<Path>) -> Result<Self> {
        KolmeStoreSqlite::new(db_path)
            .await
            .map(KolmeStore::Sqlite)
            .map_err(anyhow::Error::from)
    }

    pub async fn new_postgres(url: &str) -> Result<Self> {
        KolmeStorePostgres::new(url)
            .await
            .map(KolmeStore::Postgres)
            .map_err(anyhow::Error::from)
    }

    /// Ensures that either we have no blocks yet, or the first block has matching genesis info.
    pub(super) async fn validate_genesis_info<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
        expected: &GenesisInfo,
    ) -> Result<()> {
        if let Some(actual) = self.load_genesis_info::<AppState>(merkle_manager).await? {
            anyhow::ensure!(
                &actual == expected,
                "Mismatched genesis info.\nActual:   {actual:?}\nExpected: {expected:?}"
            );
        }
        Ok(())
    }

    async fn load_genesis_info<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<GenesisInfo>> {
        let Some(block) = self
            .load_block::<AppState>(merkle_manager, BlockHeight::start())
            .await?
        else {
            return Ok(None);
        };
        let SignedBlock::<()>(signed) = serde_json::from_str(&block.rendered)?;
        let mut messages = signed
            .message
            .into_inner()
            .tx
            .0
            .message
            .into_inner()
            .messages;
        anyhow::ensure!(messages.len() == 1);
        match messages.remove(0) {
            Message::Genesis(genesis_info) => Ok(Some(genesis_info)),
            _ => Err(anyhow::anyhow!("Invalid messages in first block")),
        }
    }

    /// Very dangerous function! Intended for testing. Deletes all blocks in the database.
    pub async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => kolme_store_sqlite.clear_blocks().await,
            KolmeStore::Postgres(kolme_store_postgres) => kolme_store_postgres.clear_blocks().await,
        }
    }
}

impl KolmeStore {
    pub async fn load_latest_block<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<StorableBlock<FrameworkState, AppState>>> {
        Ok(match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => {
                kolme_store_sqlite.load_latest_block(merkle_manager).await?
            }
            KolmeStore::Postgres(kolme_store_postgres) => {
                kolme_store_postgres
                    .load_latest_block(merkle_manager)
                    .await?
            }
        })
    }

    pub async fn load_block<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<Option<StorableBlock<FrameworkState, AppState>>> {
        let res = match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => {
                kolme_store_sqlite
                    .load_block(merkle_manager, height.0)
                    .await
            }
            KolmeStore::Postgres(kolme_store_postgres) => {
                kolme_store_postgres
                    .load_block(merkle_manager, height.0)
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
        match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => kolme_store_sqlite
                .get_height_for_tx(txhash.0)
                .await
                .map(|x| x.map(BlockHeight))
                .map_err(anyhow::Error::from),
            KolmeStore::Postgres(kolme_store_postgres) => kolme_store_postgres
                .get_height_for_tx(txhash.0)
                .await
                .map(|x| x.map(BlockHeight))
                .map_err(anyhow::Error::from),
        }
    }

    pub(super) async fn add_block<AppState: MerkleSerialize>(
        &self,
        merkle_manager: &MerkleManager,
        block: &StorableBlock<FrameworkState, AppState>,
    ) -> Result<()> {
        match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => kolme_store_sqlite
                .add_block(merkle_manager, block)
                .await
                .map_err(anyhow::Error::from),
            KolmeStore::Postgres(kolme_store_postgres) => kolme_store_postgres
                .add_block(merkle_manager, block)
                .await
                .map_err(anyhow::Error::from),
        }
    }
}
