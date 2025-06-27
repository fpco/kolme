use crate::{KolmeConstructLock, KolmeStoreError, StorableBlock, r#trait::KolmeBackingStore};
use anyhow::Context as _;
use merkle_map::{
    MerkleContents, MerkleDeserializeRaw, MerkleLayerContents, MerkleManager, MerkleSerialError,
    MerkleSerialize, MerkleSerializeRaw, MerkleStore as _, Sha256Hash,
};
use sqlx::{Executor, Postgres, pool::PoolOptions, postgres::PgAdvisoryLock};
use std::{
    collections::HashMap,
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};
mod merkle;

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

type MerkleCache = Arc<RwLock<HashMap<Sha256Hash, MerkleLayerContents>>>;

#[derive(Clone)]
pub struct Store {
    pool: sqlx::PgPool,
    merkle_cache: MerkleCache,
    latest_block: Arc<OnceLock<AtomicU64>>,
}

impl Store {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        Self::new_with_options(url, PoolOptions::new().max_connections(5)).await
    }

    pub async fn new_with_options(
        url: &str,
        options: PoolOptions<Postgres>,
    ) -> anyhow::Result<Self> {
        let pool = options
            .connect(url)
            .await
            .with_context(|| format!("Could not connect with given URL: {url}"))
            .inspect_err(|err| tracing::error!("{err:?}"))?;

        sqlx::migrate!()
            .run(&pool)
            .await
            .context("Unable to execute migrations")
            .inspect_err(|err| tracing::error!("{err:?}"))?;

        let latest_block =
            sqlx::query_scalar!("SELECT height FROM blocks ORDER BY height DESC LIMIT 1")
                .fetch_optional(&pool)
                .await
                .context("Unable to fetch latest block height from DB")
                .inspect_err(|err| tracing::error!("{err:?}"))?;

        Ok(Self {
            pool,
            latest_block: Arc::new(match latest_block {
                Some(height) => OnceLock::from(AtomicU64::new(height as u64)),
                None => OnceLock::new(),
            }),
            merkle_cache: Default::default(),
        })
    }

    pub fn new_store(&self) -> merkle::MerklePostgresStore<'_> {
        merkle::MerklePostgresStore {
            pool: &self.pool,
            merkle_cache: &self.merkle_cache,
            payloads_to_insert: Vec::new(),
            childrens_to_insert: Vec::new(),
            hashes_to_insert: Vec::new(),
        }
    }

    async fn consume_stores(
        &self,
        tx: impl Executor<'_, Database = Postgres>,
        stores: impl IntoIterator<Item = merkle::MerklePostgresStore<'_>>,
    ) -> Result<(), KolmeStoreError> {
        let mut hashes = Vec::new();
        let mut payloads = Vec::new();
        let mut childrens = Vec::new();

        for store in stores.into_iter() {
            hashes.extend(store.hashes_to_insert);
            payloads.extend(store.payloads_to_insert);
            childrens.extend(store.childrens_to_insert);
        }

        sqlx::query(
            r#"
            INSERT INTO merkle_contents(hash, payload, children)
            SELECT t.hash, t.payload, t.children
            FROM UNNEST($1::bytea[], $2::bytea[], $3::children[]) as t(hash, payload, children)
            "#,
        )
        .bind(hashes)
        .bind(payloads)
        .bind(childrens)
        .execute(tx)
        .await
        .map_err(KolmeStoreError::custom)
        .inspect_err(|err| tracing::error!("{err:?}"))?;

        Ok(())
    }

    pub async fn load_rendered_block(
        &self,
        height: u64,
    ) -> Result<Option<String>, KolmeStoreError> {
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
        .map_err(KolmeStoreError::custom)
        .inspect_err(|err| tracing::error!("{err:?}"))
    }
}

impl KolmeBackingStore for Store {
    async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        sqlx::query!("DELETE FROM blocks")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(KolmeStoreError::custom)
            .inspect_err(|err| tracing::error!("{err:?}"))
    }
    async fn delete_block(&self, _height: u64) -> Result<(), KolmeStoreError> {
        Err(KolmeStoreError::UnsupportedDeleteOperation("Postgres"))
    }

    async fn take_construct_lock(&self) -> Result<KolmeConstructLock, KolmeStoreError> {
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
            Ok(Ok(tx_unlock)) => Ok(KolmeConstructLock::Postgres {
                _lock: ConstructLock {
                    tx_unlock: Some(tx_unlock),
                },
            }),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(KolmeStoreError::custom(e)),
        }
    }

    async fn get_merkle_layer(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleLayerContents>, MerkleSerialError> {
        let mut merkle = self.new_store();
        merkle.load_by_hash(hash).await
    }
    async fn get_height_for_tx(&self, txhash: Sha256Hash) -> anyhow::Result<Option<u64>> {
        let txhash = txhash.as_array().as_slice();
        let height =
            sqlx::query_scalar!("SELECT height FROM blocks WHERE txhash=$1 LIMIT 1", txhash)
                .fetch_optional(&self.pool)
                .await
                .context("Unable to query tx height")
                .inspect_err(|err| tracing::error!("{err:?}"))?;
        match height {
            None => Ok(None),
            Some(height) => Ok(Some(height.try_into().map_err(KolmeStoreError::custom)?)),
        }
    }

    async fn load_latest_block(&self) -> Result<Option<u64>, KolmeStoreError> {
        let Some(height) = self.latest_block.get() else {
            return Ok(None);
        };

        Ok(Some(height.load(Ordering::Relaxed)))
    }
    async fn load_block<
        Block: serde::de::DeserializeOwned,
        FrameworkState: MerkleDeserializeRaw,
        AppState: MerkleDeserializeRaw,
    >(
        &self,
        merkle_manager: &MerkleManager,
        height: u64,
    ) -> Result<Option<StorableBlock<Block, FrameworkState, AppState>>, KolmeStoreError> {
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
        .map_err(KolmeStoreError::custom)
        .inspect_err(|err| tracing::error!("{err:?}"))?;

        let Some(Output {
            blockhash,
            txhash,
            rendered,
            framework_state_hash,
            app_state_hash,
            logs_hash,
        }) = output
        else {
            return Ok(None);
        };

        fn to_sha256hash(bytes: &[u8]) -> Result<Sha256Hash, KolmeStoreError> {
            Sha256Hash::from_hash(bytes).map_err(KolmeStoreError::custom)
        }

        let blockhash = to_sha256hash(&blockhash)?;
        let txhash = to_sha256hash(&txhash)?;
        let framework_state_hash = to_sha256hash(&framework_state_hash)?;
        let app_state_hash = to_sha256hash(&app_state_hash)?;
        let logs_hash = to_sha256hash(&logs_hash)?;

        let mut store1 = self.new_store();
        let mut store2 = self.new_store();
        let mut store3 = self.new_store();

        let (framework_state, app_state, logs) = tokio::try_join!(
            merkle_manager.load(&mut store1, framework_state_hash),
            merkle_manager.load(&mut store2, app_state_hash),
            merkle_manager.load(&mut store3, logs_hash),
        )?;

        let block = serde_json::from_str(&rendered).map_err(KolmeStoreError::custom)?;

        Ok(Some(StorableBlock {
            height,
            blockhash,
            txhash,
            framework_state,
            app_state,
            logs,
            block: Arc::new(block),
        }))
    }

    async fn has_block(&self, height: u64) -> Result<bool, KolmeStoreError> {
        let height_i64 = i64::try_from(height).map_err(KolmeStoreError::custom)?;
        sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM blocks WHERE height=$1)",
            height_i64,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(KolmeStoreError::custom)?
        .ok_or(KolmeStoreError::Other(
            "Impossible empty result from a SELECT EXISTS query in has_block".to_owned(),
        ))
    }
    async fn has_merkle_hash(&self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        let mut merkle = self.new_store();
        merkle.contains_hash(hash).await
    }

    async fn add_block<
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

        let mut store1 = self.new_store();
        let mut store2 = self.new_store();
        let mut store3 = self.new_store();
        let mut tx = self.pool.begin().await.map_err(KolmeStoreError::custom)?;

        let (framework_state, app_state, logs) = tokio::try_join!(
            merkle_manager.save(&mut store1, framework_state),
            merkle_manager.save(&mut store2, app_state),
            merkle_manager.save(&mut store3, logs)
        )?;

        let framework_state_hash = framework_state.hash;
        let app_state_hash = app_state.hash;
        let logs_hash = logs.hash;

        self.consume_stores(&mut *tx, [store1, store2, store3])
            .await?;

        let blockhash = blockhash.as_array().as_slice();
        let txhash = txhash.as_array().as_slice();
        let framework_state_hash = framework_state_hash.as_array().as_slice();
        let app_state_hash = app_state_hash.as_array().as_slice();
        let logs_hash = logs_hash.as_array().as_slice();
        let rendered = serde_json::to_string(&block).map_err(KolmeStoreError::custom)?;

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
        .execute(&mut *tx)
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
                    .map_err(KolmeStoreError::custom)
                    .inspect_err(|err| tracing::error!("{err:?}"))?;

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
        } else {
            tx.commit().await.map_err(KolmeStoreError::custom)?;
        }

        self.latest_block
            .get_or_init(|| AtomicU64::new(*height))
            .fetch_max(*height, Ordering::SeqCst);

        Ok(())
    }
    async fn add_merkle_layer(
        &self,
        hash: Sha256Hash,
        layer: &MerkleLayerContents,
    ) -> anyhow::Result<()> {
        let mut merkle = self.new_store();
        Ok(merkle.save_by_hash(hash, layer).await?)
    }

    async fn save<T: MerkleSerializeRaw>(
        &self,
        merkle_manager: &MerkleManager,
        value: &T,
    ) -> anyhow::Result<Arc<MerkleContents>> {
        let mut store = self.new_store();
        let contents = merkle_manager.save(&mut store, value).await?;
        self.consume_stores(&self.pool, [store]).await?;
        Ok(contents)
    }
    async fn load<T: MerkleDeserializeRaw>(
        &self,
        merkle_manager: &MerkleManager,
        hash: Sha256Hash,
    ) -> Result<T, MerkleSerialError> {
        let mut store = self.new_store();

        merkle_manager.load::<T, _>(&mut store, hash).await
    }
}
