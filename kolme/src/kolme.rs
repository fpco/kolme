use std::path::Path;

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
    pub state: Arc<tokio::sync::RwLock<KolmeState<App>>>,
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
}

pub(crate) struct EventStreamState {
    pub(crate) height: EventHeight,
    pub(crate) state: TaggedJson<RawEventState>,
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
                let state = Sha256Hash::from_hash(&state)?;
                let state = get_state_payload(pool, &state).await?;
                let state = TaggedJson::try_from_string(state)?;
                Ok(Some(EventStreamState { height, state }))
            }
        }
    }
}

pub(crate) struct ExecutionStreamState<App: KolmeApp> {
    pub(crate) height: EventHeight,
    pub(crate) framework: TaggedJson<RawExecutionState>,
    pub(crate) app: TaggedJson<App::State>,
}

impl<App: KolmeApp> ExecutionStreamState<App> {
    async fn load(pool: &sqlx::SqlitePool) -> Result<Option<ExecutionStreamState<App>>> {
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
            let framework_state=Sha256Hash::from_hash(&framework_state)?;
            let app_state=Sha256Hash::from_hash(&app_state)?;
            let height = height.try_into()?;
            let framework=get_state_payload(pool, &framework_state).await?;
            let framework=TaggedJson::try_from_string(framework)?;
            let app = get_state_payload(pool, &app_state).await?;
            let app = TaggedJson::from_pair(App::load_state(&app)?,app);
            Ok(Some(ExecutionStreamState { height, framework, app }))
        },
    }
    }
}

async fn get_state_payload(pool: &sqlx::SqlitePool, hash: &Sha256Hash) -> Result<String> {
    sqlx::query_scalar!("SELECT payload FROM state_payload WHERE hash=$1", hash)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

/// Returns the hash of the content
pub(crate) async fn insert_state_payload(
    e: &mut Transaction<'_, Sqlite>,
    payload: &[u8],
) -> Result<Sha256Hash> {
    let hash = Sha256Hash::hash(payload);
    let hash_bin = hash.0.as_slice();
    let count = sqlx::query_scalar!("SELECT COUNT(*) FROM state_payload WHERE hash=$1", hash_bin)
        .fetch_one(&mut **e)
        .await?;
    match count {
        0 => {
            sqlx::query!(
                "INSERT INTO state_payload(hash,payload) VALUES($1, $2)",
                hash_bin,
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
