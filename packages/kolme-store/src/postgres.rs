use crate::{
    r#trait::{BackingRemoteDataListener, KolmeBackingStore},
    BlockHashes, HasBlockHashes, KolmeConstructLock, KolmeStoreError, RemoteDataListener,
    StorableBlock,
};
use anyhow::Context as _;
use merkle_map::{
    MerkleDeserializeRaw, MerkleLayerContents, MerkleSerialError, MerkleSerializeRaw,
    MerkleStore as _, Sha256Hash,
};
use sqlx::{
    pool::PoolOptions,
    postgres::{PgAdvisoryLock, PgConnectOptions, PgListener},
    Executor, Postgres,
};
use std::{collections::HashMap, sync::Arc};
pub mod merkle;

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

#[derive(Clone)]
pub struct Store {
    pool: sqlx::PgPool,
    last_insert_blocks_height: Arc<tokio::sync::RwLock<Option<u64>>>,
}

impl Store {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        let connect_options = url.parse()?;
        Self::new_with_options(connect_options, PoolOptions::new().max_connections(5)).await
    }

    pub async fn new_with_options(
        connect: PgConnectOptions,
        options: PoolOptions<Postgres>,
    ) -> anyhow::Result<Self> {
        let pool = options
            .connect_with(connect)
            .await
            .context("Could not connect to the database")
            .inspect_err(|err| tracing::error!("{err:?}"))?;

        sqlx::migrate!()
            .run(&pool)
            .await
            .context("Unable to execute migrations")
            .inspect_err(|err| tracing::error!("{err:?}"))?;

        Ok(Self {
            pool,
            last_insert_blocks_height: Arc::new(tokio::sync::RwLock::new(None)),
        })
    }

    pub fn new_store(&self) -> merkle::MerklePostgresStore<'_> {
        merkle::MerklePostgresStore {
            pool: &self.pool,
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

        // NOTE: Here we use `query` instead of any of the `query!` macros
        // as they do not play well with custom `Type`/`Decode`/`Encode`
        // implementations and using Vec<u8> or [u8] types here would incur
        // in extra allocations to prepare the types
        sqlx::query(
            r#"
            INSERT INTO merkle_contents(hash, payload, children)
            SELECT t.hash, t.payload, t.children
            FROM UNNEST($1::bytea[], $2::bytea[], $3::children[]) as t(hash, payload, children)
            ON CONFLICT (hash) DO NOTHING
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
        sqlx::query!("TRUNCATE blocks")
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
        let mut dest = HashMap::new();
        merkle.load_by_hashes(&[hash], &mut dest).await?;
        Ok(dest.remove(&hash))
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
        sqlx::query_scalar!(
            r#"
            SELECT height
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
            "#
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(KolmeStoreError::custom)?
        .map(|x| u64::try_from(x).map_err(KolmeStoreError::custom))
        .transpose()
    }
    async fn load_block<Block: serde::de::DeserializeOwned>(
        &self,
        height: u64,
    ) -> Result<Option<StorableBlock<Block>>, KolmeStoreError> {
        let height_i64 = i64::try_from(height).map_err(KolmeStoreError::custom)?;
        struct Output {
            blockhash: Vec<u8>,
            txhash: Vec<u8>,
            rendered: String,
        }
        let output = sqlx::query_as!(
            Output,
            r#"
                SELECT blockhash, txhash, rendered
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
        }) = output
        else {
            return Ok(None);
        };

        fn to_sha256hash(bytes: &[u8]) -> Result<Sha256Hash, KolmeStoreError> {
            Sha256Hash::from_hash(bytes).map_err(KolmeStoreError::custom)
        }

        let blockhash = to_sha256hash(&blockhash)?;
        let txhash = to_sha256hash(&txhash)?;

        let block = serde_json::from_str(&rendered).map_err(KolmeStoreError::custom)?;

        Ok(Some(StorableBlock {
            height,
            blockhash,
            txhash,
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

    async fn add_block<Block: serde::Serialize + HasBlockHashes>(
        &self,
        StorableBlock {
            height,
            blockhash,
            txhash,
            block,
        }: &StorableBlock<Block>,
    ) -> Result<(), KolmeStoreError> {
        let height_i64 = i64::try_from(*height).map_err(KolmeStoreError::custom)?;

        let blockhash = blockhash.as_array().as_slice();
        let txhash = txhash.as_array().as_slice();
        let BlockHashes {
            framework_state,
            app_state,
            logs,
        } = block.get_block_hashes();
        let rendered = serde_json::to_string(&block).map_err(KolmeStoreError::custom)?;

        let mut last_insert_blocks_height_guard = self.last_insert_blocks_height.write().await;

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
            framework_state.as_array().as_slice(),
            app_state.as_array().as_slice(),
            logs.as_array().as_slice(),
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
                                existing: Sha256Hash::from_hash(&actualhash)
                                    .map_err(KolmeStoreError::custom)?,
                                adding: Sha256Hash::from_hash(blockhash)
                                    .map_err(KolmeStoreError::custom)?,
                            });
                        }
                    }

                    return Err(KolmeStoreError::custom(e));
                }
            }
            return Err(KolmeStoreError::custom(e));
        }

        *last_insert_blocks_height_guard = Some(*height);

        Ok(())
    }
    async fn add_merkle_layer(&self, layer: &MerkleLayerContents) -> anyhow::Result<()> {
        let mut merkle = self.new_store();
        merkle.save_by_hash(layer).await?;
        self.consume_stores(&self.pool, [merkle]).await?;

        Ok(())
    }

    async fn save<T: MerkleSerializeRaw>(&self, value: &T) -> anyhow::Result<Sha256Hash> {
        let mut store = self.new_store();
        let contents = merkle_map::save(&mut store, value).await?;
        self.consume_stores(&self.pool, [store]).await?;
        Ok(contents)
    }
    async fn load<T: MerkleDeserializeRaw>(
        &self,
        hash: Sha256Hash,
    ) -> Result<T, MerkleSerialError> {
        let mut store = self.new_store();

        merkle_map::load::<T, _>(&mut store, hash).await
    }

    async fn get_latest_archived_block_height(&self) -> anyhow::Result<Option<u64>> {
        sqlx::query_scalar!(
            r#"
            SELECT height as "height!" FROM latest_archived_block_height
            "#
        )
        .fetch_optional(&self.pool)
        .await?
        .map(|x| u64::try_from(x).map_err(anyhow::Error::from))
        .transpose()
    }

    async fn archive_block(&self, height: u64) -> anyhow::Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .context("Unable to start database")?;

        sqlx::query!(
            r#"
            INSERT INTO archived_blocks(height, archived_at)
            VALUES ($1, now())
            ON CONFLICT(height) DO UPDATE
            SET archived_at = now()
            "#,
            height as i64
        )
        .execute(&mut *tx)
        .await
        .context("Unable to store latest archived block height")?;

        sqlx::query!(
            r#"
            REFRESH MATERIALIZED VIEW latest_archived_block_height
            "#,
        )
        .execute(&mut *tx)
        .await
        .context("Unable to refresh materialized view")?;

        tx.commit()
            .await
            .context("Unable to commit archive block height changes")?;

        Ok(())
    }

    async fn listen_remote_data(&self) -> Result<Option<RemoteDataListener>, KolmeStoreError> {
        let mut listener = PgListener::connect_with(&self.pool)
            .await
            .map_err(KolmeStoreError::custom)?;
        listener
            .listen("insert_blocks")
            .await
            .map_err(KolmeStoreError::custom)?;
        Ok(Some(RemoteDataListener::from(PostgresRemoteDataListener {
            listener,
            last_insert_blocks_height: self.last_insert_blocks_height.clone(),
        })))
    }
}

pub struct PostgresRemoteDataListener {
    listener: PgListener,
    last_insert_blocks_height: Arc<tokio::sync::RwLock<Option<u64>>>,
}

impl BackingRemoteDataListener for PostgresRemoteDataListener {
    async fn recv(&mut self) {
        loop {
            match self.listener.recv().await {
                Ok(notification) => {
                    // This parses the pg_notify payload into a JSON object and extracts the height
                    // of the inserted block, then checks if it is greater than the last
                    // locally-written height.
                    let height = serde_json::from_str::<serde_json::Value>(notification.payload())
                        .ok()
                        .and_then(|v| v.get("height")?.as_str()?.parse::<u64>().ok());
                    if height.is_none() || height > *self.last_insert_blocks_height.read().await {
                        return;
                    }
                }
                Err(err) => {
                    tracing::error!("PostgreSQL insert block notification listener error: {err:?}");
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    // listener.recv() will automatically attempt to reconnect on the next call, but
                    // intervening notifications may be lost, so potentially spuriously return just
                    // to be safe.
                    return;
                }
            }
        }
    }
}
