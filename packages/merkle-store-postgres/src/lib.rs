use merkle_map::{MerkleLayerContents, MerkleSerialError, MerkleStore, Sha256Hash};
use sqlx::{pool::PoolOptions, Postgres};

pub use sqlx;
type Result<T> = std::result::Result<T, MerkleSerialError>;

#[derive(Clone)]
pub struct MerklePostgresStore {
    pool: sqlx::PgPool,
}

impl MerklePostgresStore {
    pub async fn new(url: impl AsRef<str>) -> Result<Self> {
        Self::new_with_options(url, PoolOptions::new()).await
    }

    pub async fn new_with_options(
        url: impl AsRef<str>,
        options: PoolOptions<Postgres>,
    ) -> Result<Self> {
        let pool = options
            .connect(url.as_ref())
            .await
            .map_err(MerkleSerialError::custom)?;

        sqlx::migrate!()
            .set_ignore_missing(true)
            .run(&pool)
            .await
            .map_err(MerkleSerialError::custom)?;

        Ok(Self { pool })
    }
}

impl MerkleStore for MerklePostgresStore {
    async fn load_by_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
    ) -> std::result::Result<Option<merkle_map::MerkleLayerContents>, MerkleSerialError> {
        let Some(result) = sqlx::query!(
            r#"
            SELECT payload as "payload!", children as "children!"
            FROM merkle_contents
            WHERE hash = $1
            "#,
            hash.as_array()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(MerkleSerialError::custom)?
        else {
            return Ok(None);
        };

        Ok(Some(MerkleLayerContents {
            payload: result.payload.into(),
            children: result
                .children
                .into_iter()
                .map(|hash| Sha256Hash::from_array(std::array::from_fn::<u8, 32, _>(|i| hash[i])))
                .collect(),
        }))
    }

    async fn save_by_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
        layer: &merkle_map::MerkleLayerContents,
    ) -> std::result::Result<(), MerkleSerialError> {
        let children = layer
            .children
            .clone()
            .into_iter()
            .map(|hash| hash.as_array().to_vec())
            .collect::<Vec<_>>();

        sqlx::query!(
            r#"
            INSERT INTO merkle_contents VALUES($1, $2, $3)
            "#,
            hash.as_array(),
            &*layer.payload,
            &children
        )
        .execute(&self.pool)
        .await
        .map_err(MerkleSerialError::custom)?;

        Ok(())
    }

    async fn contains_hash(
        &mut self,
        hash: merkle_map::Sha256Hash,
    ) -> std::result::Result<bool, MerkleSerialError> {
        Ok(sqlx::query!(
            r#"
            SELECT 1 as "value"
            FROM merkle_contents
            WHERE hash = $1
            "#,
            hash.as_array()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(MerkleSerialError::custom)?
        .is_some())
    }
}
