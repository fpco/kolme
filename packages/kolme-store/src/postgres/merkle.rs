use std::{collections::HashMap, sync::Arc};

use merkle_map::{CachedBytes, MerkleLayerContents, MerkleSerialError, MerkleStore, Sha256Hash};
use smallvec::SmallVec;
use sqlx::{
    encode::IsNull,
    error::BoxDynError,
    postgres::{PgHasArrayType, PgTypeInfo},
    Decode, Encode, Postgres, Type,
};

// Helper structs for sqlx serialization
pub struct Hash(Sha256Hash);

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

pub struct Payload(Arc<[u8]>);

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

pub struct ChildrenInner(SmallVec<[Sha256Hash; 16]>);

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
pub struct Children {
    bytes: ChildrenInner,
}

pub struct MerklePostgresStore<'a> {
    pub pool: &'a sqlx::PgPool,
    pub hashes_to_insert: Vec<Hash>,
    pub payloads_to_insert: Vec<Payload>,
    pub childrens_to_insert: Vec<Children>,
}

impl MerkleStore for MerklePostgresStore<'_> {
    async fn load_by_hashes(
        &mut self,
        hashes: &[Sha256Hash],
        dest: &mut HashMap<Sha256Hash, MerkleLayerContents>,
    ) -> Result<(), merkle_map::MerkleSerialError> {
        let mut to_request = vec![];
        for hash in hashes {
            to_request.push(hash.as_array().to_vec())
        }

        let query = sqlx::query!(
            r#"
            SELECT
                hash     as "hash!",
                payload  as "payload!",
                children as "children!"
            FROM merkle_contents
            WHERE hash=ANY($1)
            "#,
            &to_request,
        );

        let rows = query
            .fetch_all(self.pool)
            .await
            .map_err(merkle_map::MerkleSerialError::custom)
            .inspect_err(|err| tracing::error!("{err:?}"))?;

        for row in rows {
            let hash = Sha256Hash::from_hash(&row.hash).map_err(MerkleSerialError::custom)?;
            let children = row
                .children
                .into_iter()
                .map(|hash| Sha256Hash::from_hash(&hash).map_err(MerkleSerialError::custom))
                .collect::<Result<Vec<_>, _>>()?;
            dest.insert(
                hash,
                merkle_map::MerkleLayerContents {
                    payload: CachedBytes::new_hash(hash, row.payload),
                    children: children.into(),
                },
            );
        }
        Ok(())
    }

    async fn save_by_hash(
        &mut self,
        layer: &merkle_map::MerkleLayerContents,
    ) -> Result<(), merkle_map::MerkleSerialError> {
        let hash = layer.payload.hash();
        self.hashes_to_insert.push(Hash(hash));
        self.payloads_to_insert
            .push(Payload(layer.payload.bytes().clone()));
        self.childrens_to_insert.push(Children {
            bytes: ChildrenInner(layer.children.clone()),
        });

        Ok(())
    }

    async fn contains_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<bool, merkle_map::MerkleSerialError> {
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
