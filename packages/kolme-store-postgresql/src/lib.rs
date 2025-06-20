use kolme_store::{KolmeStoreError, StorableBlock};
use merkle_map::{MerkleDeserializeRaw, MerkleManager, MerkleSerialize, Sha256Hash};
use merkle_store::{FjallBlock, KolmeMerkleStore, Not};
use merkle_store_fjall::MerkleFjallStore;
pub use sqlx;
use sqlx::{pool::PoolOptions, postgres::PgAdvisoryLock, Postgres};
use std::{path::Path, sync::Arc};

#[derive(Clone)]
pub struct KolmeStorePostgres {
    pool: sqlx::PgPool,
    store: KolmeMerkleStore,
    fjall_block: FjallBlock,
}

const LATEST_BLOCK: &[u8] = b"latest";

impl KolmeStorePostgres {
    pub async fn new(url: &str, fjall_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let store = KolmeMerkleStore::new_fjall(fjall_dir)?;
        let fjall_block = FjallBlock::try_from_fjall_merkle_store(&store, "kolme")?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;
        sqlx::migrate!().set_ignore_missing(true).run(&pool).await?;
        Ok(Self {
            pool,
            store,
            fjall_block,
        })
    }

    pub async fn new_with_options(
        url: &str,
        options: PoolOptions<Postgres>,
        fjall_dir: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let store = KolmeMerkleStore::new_fjall(fjall_dir)?;
        let fjall_block = FjallBlock::try_from_fjall_merkle_store(&store, "kolme")?;
        let pool = options.max_connections(5).connect(url).await?;
        sqlx::migrate!().set_ignore_missing(true).run(&pool).await?;
        Ok(Self {
            pool,
            store,
            fjall_block,
        })
    }

    /// Create a PostgreSQL store with the provided merkle store
    ///
    /// **NOTE**: If you were previously using `new` and are now migrating to this method, please make
    /// sure that `fjall_dir` is the same `fjall_dir` used on `new`
    ///
    /// # Returns
    ///
    /// This function is guaranteed to error out if `fjall_dir` is not given and `store` is not a `KolmeMerkleStore::FjallStore`
    pub async fn new_with_merkle_and_fjall<Store>(
        url: &str,
        store: Store,
        fjall_dir: impl AsRef<Path>,
    ) -> anyhow::Result<Self>
    where
        Store: Into<KolmeMerkleStore> + Not<MerkleFjallStore>,
    {
        let store = store.into();
        let fjall_block = FjallBlock::try_from_options("kolme", fjall_dir.as_ref())?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await?;
        sqlx::migrate!().set_ignore_missing(true).run(&pool).await?;

        Ok(Self {
            pool,
            store,
            fjall_block,
        })
    }

    /// Create a PostgreSQL store with the provided merkle store, connection string and pool options
    ///
    /// **NOTE**: If you were previously using `new` and are now migrating to this method, please make
    /// sure that `fjall_dir` is the same `fjall_dir` used on `new`
    ///
    /// # Returns
    ///
    /// This function is guaranteed to error out if `fjall_dir` is not given and `store` is not a `KolmeMerkleStore::FjallStore`
    pub async fn new_all_settings<Store>(
        url: &str,
        options: PoolOptions<Postgres>,
        store: Store,
        fjall_dir: impl AsRef<Path>,
    ) -> anyhow::Result<Self>
    where
        Store: Into<KolmeMerkleStore> + Not<MerkleFjallStore>,
    {
        let store = store.into();
        let fjall_block = FjallBlock::try_from_options("kolme", fjall_dir.as_ref())?;
        let pool = options.max_connections(5).connect(url).await?;
        sqlx::migrate!().set_ignore_missing(true).run(&pool).await?;

        Ok(Self {
            pool,
            store,
            fjall_block,
        })
    }

    pub async fn load_latest_block(&self) -> Result<Option<u64>, KolmeStoreError> {
        let latest = self
            .fjall_block
            .get(LATEST_BLOCK)
            .map_err(KolmeStoreError::custom)?;
        let Some(latest) = latest else {
            return Ok(None);
        };
        let latest = <[u8; 8]>::try_from(&*latest).map_err(KolmeStoreError::custom)?;
        Ok(Some(u64::from_be_bytes(latest)))
    }

    pub async fn load_block<
        Block: serde::de::DeserializeOwned,
        FrameworkState: MerkleDeserializeRaw,
        AppState: MerkleDeserializeRaw,
    >(
        &self,
        merkle_manager: &MerkleManager,
        height: u64,
    ) -> Result<StorableBlock<Block, FrameworkState, AppState>, KolmeStoreError> {
        let height_i64 = i64::try_from(height).map_err(KolmeStoreError::custom)?;
        struct Output {
            blockhash: Vec<u8>,
            txhash: Vec<u8>,
            rendered: String,
            framework_state_hash: Vec<u8>,
            app_state_hash: Vec<u8>,
            logs_hash: Vec<u8>,
        }
        let output = sqlx::query_as!(
            Output,
            r#"
                SELECT
                    blockhash, txhash, rendered,
                    framework_state_hash, app_state_hash, logs_hash
                FROM blocks
                WHERE height=$1
                LIMIT 1
            "#,
            height_i64,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(KolmeStoreError::custom)?;
        let Some(Output {
            blockhash,
            txhash,
            rendered,
            framework_state_hash,
            app_state_hash,
            logs_hash,
        }) = output
        else {
            return Err(KolmeStoreError::BlockNotFound { height });
        };

        fn to_sha256hash(bytes: &[u8]) -> Result<Sha256Hash, KolmeStoreError> {
            Sha256Hash::from_hash(bytes).map_err(KolmeStoreError::custom)
        }

        let blockhash = to_sha256hash(&blockhash)?;
        let txhash = to_sha256hash(&txhash)?;
        let framework_state_hash = to_sha256hash(&framework_state_hash)?;
        let app_state_hash = to_sha256hash(&app_state_hash)?;
        let logs_hash = to_sha256hash(&logs_hash)?;

        let mut store1 = self.store.clone();
        let mut store2 = self.store.clone();
        let mut store3 = self.store.clone();

        let merkle_res = tokio::try_join!(
            merkle_manager.load(&mut store1, framework_state_hash),
            merkle_manager.load(&mut store2, app_state_hash),
            merkle_manager.load(&mut store3, logs_hash),
        );

        let (framework_state, app_state, logs) = merkle_res?;

        let block = serde_json::from_str(&rendered).map_err(KolmeStoreError::custom)?;

        Ok(StorableBlock {
            height,
            blockhash,
            txhash,
            framework_state,
            app_state,
            logs,
            block: Arc::new(block),
        })
    }

    pub async fn load_rendered_block(&self, height: u64) -> Result<String, KolmeStoreError> {
        let height_i64 = i64::try_from(height).map_err(KolmeStoreError::custom)?;
        sqlx::query_scalar!(
            r#"
                SELECT rendered
                FROM blocks
                WHERE height=$1
                LIMIT 1
            "#,
            height_i64,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(KolmeStoreError::custom)?
        .ok_or(KolmeStoreError::BlockNotFound { height })
    }

    pub async fn add_block<
        Block: serde::Serialize,
        FrameworkState: MerkleSerialize,
        AppState: MerkleSerialize,
    >(
        &self,
        merkle_manager: &MerkleManager,
        StorableBlock {
            height,
            blockhash,
            txhash,
            framework_state,
            app_state,
            logs,
            block,
        }: &StorableBlock<Block, FrameworkState, AppState>,
    ) -> Result<(), KolmeStoreError> {
        let height_i64 = i64::try_from(*height).map_err(KolmeStoreError::custom)?;

        let mut store = self.store.clone();
        let framework_state_hash = merkle_manager.save(&mut store, framework_state).await?.hash;
        let app_state_hash = merkle_manager.save(&mut store, app_state).await?.hash;
        let logs_hash = merkle_manager.save(&mut store, logs).await?.hash;

        let blockhash = blockhash.as_array().as_slice();
        let txhash = txhash.as_array().as_slice();
        let framework_state_hash = framework_state_hash.as_array().as_slice();
        let app_state_hash = app_state_hash.as_array().as_slice();
        let logs_hash = logs_hash.as_array().as_slice();
        let rendered = serde_json::to_string(block).map_err(KolmeStoreError::custom)?;

        let res = sqlx::query!(
            r#"
                INSERT INTO
                blocks(height, blockhash, rendered, txhash, framework_state_hash, app_state_hash, logs_hash)
                VALUES($1, $2, $3, $4, $5, $6, $7)
            "#,
            height_i64,
            blockhash,
            rendered,
            txhash,
            framework_state_hash,
            app_state_hash,
            logs_hash,
        )
        .execute(&self.pool)
        .await;
        if let Err(e) = res {
            // If the block already exists in the database, ignore the error
            if let Some(db_error) = e.as_database_error() {
                if db_error.code().as_deref() == Some("23505") {
                    let actualhash = sqlx::query_scalar!(
                        r#"
                            SELECT blockhash FROM blocks
                            WHERE height=$1
                            LIMIT 1
                        "#,
                        height_i64,
                    )
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(KolmeStoreError::custom)?;

                    if let Some(actualhash) = actualhash {
                        if actualhash == blockhash {
                            return Err(KolmeStoreError::MatchingBlockAlreadyInserted {
                                height: *height,
                            });
                        } else {
                            return Err(KolmeStoreError::ConflictingBlockInDb {
                                height: *height,
                                hash: Sha256Hash::from_hash(&actualhash)
                                    .map_err(KolmeStoreError::custom)?,
                            });
                        }
                    }

                    return Err(KolmeStoreError::custom(e));
                }
            }
            return Err(KolmeStoreError::custom(e));
        }

        // Update the latest within Fjall
        let old = self.load_latest_block().await?;
        let to_store = match old {
            None => true,
            Some(old) => old < *height,
        };
        if to_store {
            self.fjall_block
                .insert(LATEST_BLOCK, height.to_be_bytes())
                .map_err(KolmeStoreError::custom)?;
        }
        Ok(())
    }

    pub async fn get_height_for_tx(
        &self,
        txhash: Sha256Hash,
    ) -> Result<Option<u64>, KolmeStoreError> {
        let txhash = txhash.as_array().as_slice();
        let height =
            sqlx::query_scalar!("SELECT height FROM blocks WHERE txhash=$1 LIMIT 1", txhash)
                .fetch_optional(&self.pool)
                .await
                .map_err(KolmeStoreError::custom)?;
        match height {
            None => Ok(None),
            Some(height) => Ok(Some(height.try_into().map_err(KolmeStoreError::custom)?)),
        }
    }

    pub async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        sqlx::query!("DELETE FROM blocks")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(KolmeStoreError::custom)
    }

    pub async fn take_construct_lock(&self) -> Result<ConstructLock, KolmeStoreError> {
        let (tx_locked, rx_locked) = tokio::sync::oneshot::channel();
        let conn = self.pool.acquire().await.map_err(KolmeStoreError::custom)?;
        let lock = PgAdvisoryLock::new("construct");
        tokio::spawn(async move {
            match lock.acquire(conn).await.map_err(KolmeStoreError::custom) {
                Ok(guard) => {
                    let (tx_unlock, rx_unlock) = tokio::sync::oneshot::channel::<()>();
                    if tx_locked.send(Ok(tx_unlock)).is_ok() {
                        rx_unlock.await.ok();
                    }
                    if let Err(e) = guard.release_now().await {
                        tracing::error!("Error releasing PostgreSQL construction lock: {e}");
                    }
                }
                Err(e) => {
                    tx_locked.send(Err(e)).ok();
                }
            }
        });
        match rx_locked.await {
            Ok(Ok(tx_unlock)) => Ok(ConstructLock {
                tx_unlock: Some(tx_unlock),
            }),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(KolmeStoreError::custom(e)),
        }
    }

    pub fn get_merkle_store(&self) -> &KolmeMerkleStore {
        &self.store
    }
}

pub struct ConstructLock {
    tx_unlock: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for ConstructLock {
    fn drop(&mut self) {
        if self.tx_unlock.take().unwrap().send(()).is_err() {
            tracing::error!("Error while dropping a PostgreSQL construct lock");
        }
    }
}
