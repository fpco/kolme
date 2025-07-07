use super::{core::RawMerkleMap, r#trait::StoreEnv};
use kolme::{MerkleLayerContents, Sha256Hash};
use merkle::MerklePostgresStore;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub struct StoreOptions {
    pub url: String,
}

type MerkleCache = Arc<RwLock<HashMap<Sha256Hash, MerkleLayerContents>>>;

#[derive(Clone)]
pub struct Store {
    pool: sqlx::PgPool,
    merkle_cache: MerkleCache,
}

impl Store {
    async fn consume_merkle<'a>(merkle_store: MerklePostgresStore<'a>) {
        let hashes = merkle_store.hashes_to_insert;
        let payloads = merkle_store.payloads_to_insert;
        let childrens = merkle_store.childrens_to_insert;

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

        sqlx::migrate!().set_ignore_missing(true).run(&pool).await.expect("Unable to complete migrations");

        Store {
            pool,
            merkle_cache: Arc::new(RwLock::new(Default::default())),
        }
    }

    async fn run(&mut self, manager: &kolme::MerkleManager, map: RawMerkleMap) {
        let mut merkle_store = merkle::MerklePostgresStore {
            pool: &self.pool,
            merkle_cache: &self.merkle_cache,
            hashes_to_insert: Vec::new(),
            payloads_to_insert: Vec::new(),
            childrens_to_insert: Vec::new(),
        };

        manager
            .save(&mut merkle_store, &map.0)
            .await
            .expect("Unable to save MekrleMap");

        Self::consume_merkle(merkle_store).await;
    }

    async fn cleanup(&mut self) {
        sqlx::query!(
            r#"
            TRUNCATE TABLE merkle_contents
            "#,
        )
        .execute(&self.pool)
        .await
        .expect("Unable to insert MerkleMap hash contents");
    }
}

mod merkle {
    use std::sync::Arc;

    use merkle_map::{MerkleStore, Sha256Hash};
    use smallvec::SmallVec;
    use sqlx::{
        Decode, Encode, Postgres, Type,
        encode::IsNull,
        error::BoxDynError,
        postgres::{PgHasArrayType, PgTypeInfo},
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
            let slice: &[Sha256Hash] = &self.0;
            let slice: &[[u8; 32]] = unsafe { std::mem::transmute(slice) };

            slice.encode_by_ref(buf)
        }
    }

    impl<'a> Decode<'a, Postgres> for ChildrenInner {
        fn decode(_: <Postgres as sqlx::Database>::ValueRef<'a>) -> Result<Self, BoxDynError> {
            unreachable!(
                "This should should not be called as this struct is not used for deserialization"
            )
        }
    }

    #[derive(sqlx::Type)]
    #[sqlx(type_name = "children")]
    pub(super) struct Children {
        bytes: ChildrenInner,
    }

    pub struct MerklePostgresStore<'a> {
        pub(super) pool: &'a sqlx::PgPool,
        pub(super) merkle_cache: &'a MerkleCache,
        pub(super) hashes_to_insert: Vec<Hash>,
        pub(super) payloads_to_insert: Vec<Payload>,
        pub(super) childrens_to_insert: Vec<Children>,
    }

    impl MerkleStore for MerklePostgresStore<'_> {
        async fn load_by_hash(
            &mut self,
            hash: Sha256Hash,
        ) -> Result<Option<merkle_map::MerkleLayerContents>, merkle_map::MerkleSerialError>
        {
            if let Some(contents) = self.merkle_cache.read().get(&hash) {
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
            .map_err(merkle_map::MerkleSerialError::custom)
            .inspect_err(|err| tracing::error!("{err:?}"))?
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
            self.childrens_to_insert.push(Children {
                bytes: ChildrenInner(layer.children.clone()),
            });

            self.merkle_cache.write().insert(hash, layer.clone());

            Ok(())
        }

        async fn contains_hash(
            &mut self,
            hash: Sha256Hash,
        ) -> Result<bool, merkle_map::MerkleSerialError> {
            if self.merkle_cache.read().contains_key(&hash) {
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
            .map_err(merkle_map::MerkleSerialError::custom)
            .inspect_err(|err| tracing::error!("{err:?}"))?
            .is_some())
        }
    }
}
