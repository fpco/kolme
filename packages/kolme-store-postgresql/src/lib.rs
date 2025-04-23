mod merkle_db_store;

use kolme_store::{KolmeStoreError, StorableBlock};
use merkle_db_store::MerkleDbStore;
use merkle_map::{MerkleDeserialize, MerkleManager, MerkleSerialize, Sha256Hash};

pub struct KolmeStorePostgres(sqlx::PgPool);

impl KolmeStorePostgres {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        let pool = sqlx::PgPool::connect(url).await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Self(pool))
    }

    pub async fn load_latest_block<
        FrameworkState: MerkleDeserialize,
        AppState: MerkleDeserialize,
    >(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<StorableBlock<FrameworkState, AppState>>, KolmeStoreError> {
        let height = sqlx::query_scalar!("SELECT height FROM blocks ORDER BY height DESC LIMIT 1")
            .fetch_optional(&self.0)
            .await
            .map_err(KolmeStoreError::custom)?;
        match height {
            None => Ok(None),
            Some(height) => self
                .load_block(
                    merkle_manager,
                    height.try_into().map_err(KolmeStoreError::custom)?,
                )
                .await
                .map(Some),
        }
    }

    pub async fn load_block<FrameworkState: MerkleDeserialize, AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
        height: u64,
    ) -> Result<StorableBlock<FrameworkState, AppState>, KolmeStoreError> {
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
        .fetch_optional(&self.0)
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

        let mut store = MerkleDbStore::Pool(&self.0);
        let framework_state = merkle_manager
            .load(&mut store, framework_state_hash)
            .await?;
        let app_state = merkle_manager.load(&mut store, app_state_hash).await?;
        let logs = merkle_manager.load(&mut store, logs_hash).await?;

        Ok(StorableBlock {
            height,
            blockhash,
            txhash,
            rendered,
            framework_state,
            app_state,
            logs,
        })
    }

    pub async fn add_block<FrameworkState: MerkleSerialize, AppState: MerkleSerialize>(
        &self,
        merkle_manager: &MerkleManager,
        StorableBlock {
            height,
            blockhash,
            txhash,
            rendered,
            framework_state,
            app_state,
            logs,
        }: &StorableBlock<FrameworkState, AppState>,
    ) -> Result<(), KolmeStoreError> {
        let height_i64 = i64::try_from(*height).map_err(KolmeStoreError::custom)?;

        let mut trans = self.0.begin().await.map_err(KolmeStoreError::custom)?;
        let mut store = MerkleDbStore::Conn(&mut trans);
        let framework_state_hash = merkle_manager.save(&mut store, framework_state).await?.hash;
        let app_state_hash = merkle_manager.save(&mut store, app_state).await?.hash;
        let logs_hash = merkle_manager.save(&mut store, logs).await?.hash;

        let blockhash = blockhash.as_array().as_slice();
        let txhash = txhash.as_array().as_slice();
        let framework_state_hash = framework_state_hash.as_array().as_slice();
        let app_state_hash = app_state_hash.as_array().as_slice();
        let logs_hash = logs_hash.as_array().as_slice();

        sqlx::query!(
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
        .execute(&mut *trans)
        .await.map_err(KolmeStoreError::custom)?;
        trans.commit().await.map_err(KolmeStoreError::custom)?;
        Ok(())
    }

    pub async fn get_height_for_tx(
        &self,
        txhash: Sha256Hash,
    ) -> Result<Option<u64>, KolmeStoreError> {
        let txhash = txhash.as_array().as_slice();
        let height =
            sqlx::query_scalar!("SELECT height FROM blocks WHERE txhash=$1 LIMIT 1", txhash)
                .fetch_optional(&self.0)
                .await
                .map_err(KolmeStoreError::custom)?;
        match height {
            None => Ok(None),
            Some(height) => Ok(Some(height.try_into().map_err(KolmeStoreError::custom)?)),
        }
    }

    pub async fn clear_blocks(&self) -> Result<(), KolmeStoreError> {
        sqlx::query!("DELETE FROM blocks")
            .execute(&self.0)
            .await
            .map(|_| ())
            .map_err(KolmeStoreError::custom)
    }
}
