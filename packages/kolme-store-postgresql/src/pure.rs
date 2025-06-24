use anyhow::Context as _;
use kolme_store::{KolmeStoreError, StorableBlock};
use merkle_map::{
    MerkleContents, MerkleDeserializeRaw, MerkleLayerContents, MerkleManager, MerkleSerialize,
    Sha256Hash,
};
use sqlx::{pool::PoolOptions, postgres::PgAdvisoryLock, Postgres};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use crate::ConstructLock;

type MerkleCache = Arc<RwLock<HashMap<Sha256Hash, MerkleLayerContents>>>;

#[derive(Clone)]
pub struct KolmeStorePurePostgres {
    pool: sqlx::PgPool,
    merkle_cache: MerkleCache,
    latest_block: Arc<AtomicU64>,
}

impl KolmeStorePurePostgres {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        Self::new_with_options(url, PoolOptions::new()).await
    }

    pub async fn new_with_options(
        url: &str,
        options: PoolOptions<Postgres>,
    ) -> anyhow::Result<Self> {
        let pool = options
            .connect(url)
            .await
            .with_context(|| format!("Could not connect with given URL: {url}"))?;

        sqlx::migrate!()
            .run(&pool)
            .await
            .context("Unable to execute migrations")?;

        let latest_block =
            sqlx::query_scalar!("SELECT height FROM blocks ORDER BY height DESC LIMIT 1")
                .fetch_optional(&pool)
                .await
                .context("Unable to fetch latest block height from DB")?
                .unwrap_or_default();

        Ok(Self {
            pool,
            latest_block: Arc::new(AtomicU64::new(latest_block as u64)),
            merkle_cache: Default::default(),
        })
    }

    pub fn load_latest_block(&self) -> Result<Option<u64>, KolmeStoreError> {
        Ok(Some(self.latest_block.load(Ordering::Relaxed)))
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

        let mut store1 = self.new_store();
        let mut store2 = self.new_store();
        let mut store3 = self.new_store();

        let (framework_state, app_state, logs) = tokio::try_join!(
            merkle_manager.load(&mut store1, framework_state_hash),
            merkle_manager.load(&mut store2, app_state_hash),
            merkle_manager.load(&mut store3, logs_hash),
        )?;

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

        let mut store1 = self.new_store();
        let mut store2 = self.new_store();
        let mut store3 = self.new_store();

        let (framework_state, app_state, logs) = tokio::try_join!(
            merkle_manager.save(&mut store1, framework_state),
            merkle_manager.save(&mut store2, app_state),
            merkle_manager.save(&mut store3, logs)
        )?;

        let framework_state_hash = framework_state.hash;
        let app_state_hash = app_state.hash;
        let logs_hash = logs.hash;

        self.consume_stores([store1, store2, store3]).await?;

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

        self.latest_block.fetch_max(*height, Ordering::SeqCst);

        Ok(())
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

    pub async fn store_and_load<T: MerkleDeserializeRaw>(
        &self,
        merkle_manager: &MerkleManager,
        contents: &MerkleContents,
    ) -> Result<T, KolmeStoreError> {
        let mut store = self.new_store();

        merkle_manager
            .save_merkle_contents(&mut store, contents)
            .await?;
        let result = merkle_manager
            .load::<T, _>(&mut store, contents.hash)
            .await
            .map_err(KolmeStoreError::custom)?;

        self.consume_stores([store]).await?;

        Ok(result)
    }

    fn new_store(&self) -> merkle::PurePostgresMerkleStore<'_> {
        merkle::PurePostgresMerkleStore {
            pool: &self.pool,
            merkle_cache: &self.merkle_cache,
            payloads_to_insert: Vec::new(),
            childrens_to_insert: Vec::new(),
            hashes_to_insert: Vec::new(),
        }
    }

    async fn consume_stores(
        &self,
        stores: impl IntoIterator<Item = merkle::PurePostgresMerkleStore<'_>>,
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
        .execute(&self.pool)
        .await
        .map_err(KolmeStoreError::custom)?;

        Ok(())
    }
}

mod merkle {
    use std::sync::Arc;

    use merkle_map::{MerkleStore, Sha256Hash};
    use smallvec::SmallVec;
    use sqlx::{
        encode::IsNull,
        error::BoxDynError,
        postgres::{types::PgRecordEncoder, PgHasArrayType, PgTypeInfo},
        Encode, Postgres, Type,
    };

    use super::MerkleCache;

    // Helper structs for sqlx serialization
    pub(super) struct Hash(Sha256Hash);

    impl Type<Postgres> for Hash {
        fn type_info() -> <Postgres as sqlx::Database>::TypeInfo {
            PgTypeInfo::with_name("bytea")
        }
    }

    impl<'a> Encode<'a, Postgres> for Hash {
        fn encode_by_ref(
            &self,
            buf: &mut <Postgres as sqlx::Database>::ArgumentBuffer<'a>,
        ) -> Result<IsNull, BoxDynError> {
            buf.extend_from_slice(self.0.as_array());

            Ok(IsNull::No)
        }
    }

    impl PgHasArrayType for Hash {
        fn array_type_info() -> PgTypeInfo {
            PgTypeInfo::array_of("bytea")
        }
    }

    pub(super) struct Payload(Arc<[u8]>);

    impl Type<Postgres> for Payload {
        fn type_info() -> <Postgres as sqlx::Database>::TypeInfo {
            PgTypeInfo::with_name("bytea")
        }
    }

    impl PgHasArrayType for Payload {
        fn array_type_info() -> PgTypeInfo {
            PgTypeInfo::array_of("bytea")
        }
    }

    impl<'a> Encode<'a, Postgres> for Payload {
        fn encode_by_ref(
            &self,
            buf: &mut <Postgres as sqlx::Database>::ArgumentBuffer<'a>,
        ) -> Result<IsNull, BoxDynError> {
            buf.extend_from_slice(&self.0);

            Ok(IsNull::No)
        }
    }

    pub(super) struct ChildrenInner(SmallVec<[Sha256Hash; 16]>);

    impl Type<Postgres> for ChildrenInner {
        fn type_info() -> <Postgres as sqlx::Database>::TypeInfo {
            PgTypeInfo::array_of("bytea")
        }
    }

    impl<'a> Encode<'a, Postgres> for ChildrenInner {
        fn encode_by_ref(
            &self,
            buf: &mut <Postgres as sqlx::Database>::ArgumentBuffer<'a>,
        ) -> Result<IsNull, BoxDynError> {
            for hash in self.0.iter() {
                buf.extend_from_slice(hash.as_array());
            }

            Ok(IsNull::No)
        }
    }

    pub(super) struct Children(ChildrenInner);

    impl Type<Postgres> for Children {
        fn type_info() -> <Postgres as sqlx::Database>::TypeInfo {
            PgTypeInfo::with_name("children")
        }
    }

    impl<'a> Encode<'a, Postgres> for Children {
        fn encode_by_ref(
            &self,
            buf: &mut <Postgres as sqlx::Database>::ArgumentBuffer<'a>,
        ) -> Result<IsNull, BoxDynError> {
            let mut encoder = PgRecordEncoder::new(buf);
            encoder.encode(&self.0)?;
            encoder.finish();
            Ok(IsNull::No)
        }
    }

    impl PgHasArrayType for Children {
        fn array_type_info() -> PgTypeInfo {
            PgTypeInfo::array_of("bytea[]")
        }
    }

    pub struct PurePostgresMerkleStore<'a> {
        pub(super) pool: &'a sqlx::PgPool,
        pub(super) merkle_cache: &'a MerkleCache,
        pub(super) hashes_to_insert: Vec<Hash>,
        pub(super) payloads_to_insert: Vec<Payload>,
        pub(super) childrens_to_insert: Vec<Children>,
    }

    impl MerkleStore for PurePostgresMerkleStore<'_> {
        async fn load_by_hash(
            &mut self,
            hash: Sha256Hash,
        ) -> Result<Option<merkle_map::MerkleLayerContents>, merkle_map::MerkleSerialError>
        {
            if let Some(contents) = self.merkle_cache.read().unwrap().get(&hash) {
                return Ok(Some(contents.clone()));
            }

            Ok(sqlx::query!(
                r#"
                SELECT
                    payload  as "payload!",
                    children as "children!"
                FROM merkle_contents
                WHERE hash = $1
                "#,
                hash.as_array()
            )
            .fetch_optional(self.pool)
            .await
            .map_err(merkle_map::MerkleSerialError::custom)?
            .map(|row| merkle_map::MerkleLayerContents {
                payload: row.payload.into(),
                children: row
                    .children
                    .into_iter()
                    .map(|hash| {
                        let hash_array = std::array::from_fn::<u8, 32, _>(|i| hash[i]);
                        Sha256Hash::from_array(hash_array)
                    })
                    .collect(),
            }))
        }

        async fn save_by_hash(
            &mut self,
            hash: Sha256Hash,
            layer: &merkle_map::MerkleLayerContents,
        ) -> Result<(), merkle_map::MerkleSerialError> {
            self.hashes_to_insert.push(Hash(hash));
            self.payloads_to_insert.push(Payload(layer.payload.clone()));
            self.childrens_to_insert
                .push(Children(ChildrenInner(layer.children.clone())));

            self.merkle_cache
                .write()
                .unwrap()
                .insert(hash, layer.clone());

            Ok(())
        }

        async fn contains_hash(
            &mut self,
            hash: Sha256Hash,
        ) -> Result<bool, merkle_map::MerkleSerialError> {
            if self.merkle_cache.read().unwrap().contains_key(&hash) {
                return Ok(true);
            }

            Ok(sqlx::query!(
                r#"
                SELECT 1 as "value!"
                FROM merkle_contents
                WHERE hash = $1
                "#,
                hash.as_array()
            )
            .fetch_optional(self.pool)
            .await
            .map_err(merkle_map::MerkleSerialError::custom)?
            .is_some())
        }
    }
}
