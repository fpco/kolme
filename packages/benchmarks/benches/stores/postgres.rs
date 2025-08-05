use super::{core::RawMerkleMap, r#trait::StoreEnv};
use kolme::{MerkleLayerContents, Sha256Hash};
use kolme_store::postgres::merkle::MerklePostgresStore;
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;

#[derive(Clone)]
pub struct StoreOptions {
    pub url: String,
}

type MerkleCache = Arc<Mutex<LruCache<Sha256Hash, MerkleLayerContents>>>;

#[derive(Clone)]
pub struct Store {
    pool: sqlx::PgPool,
    merkle_cache: MerkleCache,
}

impl Store {
    async fn consume_merkle(merkle_store: MerklePostgresStore<'_>) {
        let hashes = merkle_store.hashes_to_insert;
        let payloads = merkle_store.payloads_to_insert;
        let childrens = merkle_store.childrens_to_insert;

        sqlx::query(
            r#"
            INSERT INTO bench_merkle_contents(hash, payload, children)
            SELECT t.hash, t.payload, t.children
            FROM UNNEST($1::bytea[], $2::bytea[], $3::children[]) as t(hash, payload, children)
            ON CONFLICT (hash) DO NOTHING
            "#,
        )
        .bind(hashes)
        .bind(payloads)
        .bind(childrens)
        .execute(merkle_store.pool)
        .await
        .expect("Unable to insert MerkleMap hash contents");
    }
}

impl StoreEnv for Store {
    type Params = StoreOptions;

    async fn new(params: Self::Params) -> Self {
        let pool = sqlx::PgPool::connect(&params.url)
            .await
            .expect("Unable to connect to postgres");

        sqlx::migrate!()
            .set_ignore_missing(true)
            .run(&pool)
            .await
            .expect("Unable to complete migrations");

        Store {
            pool,
            merkle_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(4096).unwrap()))),
        }
    }

    async fn run(&mut self, map: RawMerkleMap) {
        let mut merkle_store = MerklePostgresStore {
            pool: &self.pool,
            merkle_cache: &self.merkle_cache,
            hashes_to_insert: Vec::new(),
            payloads_to_insert: Vec::new(),
            childrens_to_insert: Vec::new(),
        };

        merkle_map::save(&mut merkle_store, &map.0)
            .await
            .expect("Unable to save MekrleMap");

        Self::consume_merkle(merkle_store).await;
    }

    async fn cleanup(&mut self) {
        sqlx::query!(
            r#"
            TRUNCATE TABLE bench_merkle_contents
            "#,
        )
        .execute(&self.pool)
        .await
        .expect("Unable to insert MerkleMap hash contents");
    }
}
