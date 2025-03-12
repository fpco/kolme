use std::path::Path;

use k256::sha2::{Digest, Sha256};
use sqlx::{sqlite::SqliteConnectOptions, Sqlite, Transaction};

use crate::*;

pub struct Kolme<App: KolmeApp> {
    pub inner: Arc<KolmeInner<App>>,
}

impl<App: KolmeApp> Clone for Kolme<App> {
    fn clone(&self) -> Self {
        Kolme {
            inner: self.inner.clone(),
        }
    }
}

pub struct KolmeInner<App: KolmeApp> {
    pub pool: sqlx::SqlitePool,
    pub(crate) state: Arc<tokio::sync::RwLock<KolmeState<App>>>,
    pub app: App,
}

impl<App: KolmeApp> Kolme<App> {
    pub async fn new(
        app: App,
        code_version: impl AsRef<str>,
        db_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let code_version = code_version.as_ref();
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(options).await?;
        sqlx::migrate!().run(&pool).await?;
        let event = EventStreamState::load(&pool).await?;
        let exec = ExecutionStreamState::load(&pool).await?;
        let state = KolmeState::new(&app, event, exec, code_version).await?;
        Ok(Kolme {
            inner: Arc::new(KolmeInner {
                pool,
                state: Arc::new(tokio::sync::RwLock::new(state)),
                app,
            }),
        })
    }

    pub async fn get_next_event_height(&self) -> EventHeight {
        self.inner.state.read().await.event.get_next_height()
    }

    pub async fn get_next_exec_height(&self) -> EventHeight {
        self.inner.state.read().await.exec.get_next_height()
    }

    pub(crate) async fn get_next_account_nonce(&self, public_key: k256::PublicKey) -> AccountNonce {
        let guard = self.inner.state.read().await;
        match guard.event.get_account_id(&public_key) {
            None => AccountNonce::start(),
            Some(account_id) => guard.event.get_next_nonce(account_id).unwrap(),
        }
    }
}

pub(crate) struct EventStreamState {
    pub(crate) height: EventHeight,
    pub(crate) state: Vec<u8>,
}

impl EventStreamState {
    async fn load(pool: &sqlx::SqlitePool) -> Result<Option<EventStreamState>> {
        struct Helper {
            height: i64,
            state: Vec<u8>,
        }
        match sqlx::query_as!(
            Helper,
            "SELECT height,state FROM event_stream ORDER BY height DESC LIMIT 1"
        )
        .fetch_optional(pool)
        .await?
        {
            None => Ok(None),
            Some(Helper { height, state }) => {
                let height = height.try_into()?;
                let state = get_state_payload(pool, &state).await?;
                Ok(Some(EventStreamState { height, state }))
            }
        }
    }
}

pub(crate) struct ExecutionStreamState {
    pub(crate) height: EventHeight,
    pub(crate) framework: Vec<u8>,
    pub(crate) app: Vec<u8>,
}

impl ExecutionStreamState {
    async fn load(pool: &sqlx::SqlitePool) -> Result<Option<ExecutionStreamState>> {
        #[derive(serde::Deserialize)]
        struct Helper {
            height: i64,
            framework_state: Vec<u8>,
            app_state: Vec<u8>,
        }
        match sqlx::query_as!(
        Helper,
        "SELECT height, framework_state, app_state FROM execution_stream ORDER BY height DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await? {
        None => Ok(None),
        Some(Helper { height, framework_state, app_state  }) => {
            let height = height.try_into()?;
            let framework=get_state_payload(pool, &framework_state).await?;
            let app = get_state_payload(pool, &app_state).await?;
            Ok(Some(ExecutionStreamState { height, framework, app }))
        },
    }
    }
}

async fn get_state_payload(pool: &sqlx::SqlitePool, hash: &[u8]) -> Result<Vec<u8>> {
    Ok(
        sqlx::query_scalar!("SELECT payload FROM state_payload WHERE hash=$1", hash)
            .fetch_one(pool)
            .await?,
    )
}

/// Returns the hash of the content
pub(crate) async fn insert_state_payload(
    e: &mut Transaction<'_, Sqlite>,
    payload: &[u8],
) -> Result<Vec<u8>> {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let hash = hasher.finalize().to_vec();
    let count = sqlx::query_scalar!("SELECT COUNT(*) FROM state_payload WHERE hash=$1", hash)
        .fetch_one(&mut **e)
        .await?;
    match count {
        0 => {
            sqlx::query!(
                "INSERT INTO state_payload(hash,payload) VALUES($1, $2)",
                hash,
                payload
            )
            .execute(&mut **e)
            .await?;
        }
        1 => (),
        _ => anyhow::bail!("insert_state_payload: impossible result of {count} entries"),
    }
    Ok(hash)
}
