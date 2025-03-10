use std::path::Path;

use sqlx::sqlite::SqliteConnectOptions;

use crate::{framework_state::FrameworkState, prelude::*};

pub struct Kolme<App> {
    pub pool: sqlx::SqlitePool,
    pub framework_state: Arc<RwLock<FrameworkState>>,
    pub app: App,
}

impl<App: KolmeApp> Kolme<App> {
    pub(crate) async fn new(
        app: App,
        code_version: impl AsRef<str>,
        db_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(options).await?;
        sqlx::migrate!().run(&pool).await?;
        let framework_state = match get_latest_state(&pool).await? {
            LatestState::NoState => FrameworkState::new(App::initial_framework_state())?,
            LatestState::Latest {
                last_event_height,
                last_state_height,
                framework_state,
                app_state,
            } => {
                FrameworkState::load(&app, last_event_height, last_state_height, &framework_state)
                    .await?
            }
        };
        framework_state.validate_code_version(code_version)?;
        Ok(Kolme {
            pool,
            framework_state: Arc::new(RwLock::new(framework_state)),
            app,
        })
    }
}

enum LatestState {
    NoState,
    Latest {
        last_event_height: EventHeight,
        last_state_height: EventHeight,
        framework_state: Vec<u8>,
        app_state: Vec<u8>,
    },
}

async fn get_latest_state(pool: &sqlx::SqlitePool) -> Result<LatestState> {
    #[derive(serde::Deserialize)]
    struct Helper {
        height: i64,
        framework_state_hash: Vec<u8>,
        app_state_hash: Vec<u8>,
    }
    match sqlx::query_as!(
        Helper,
        "SELECT height, framework_state_hash, app_state_hash FROM state_stream ORDER BY height DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await? {
        Some(Helper { height, framework_state_hash, app_state_hash }) => {
            let last_state_height = EventHeight(height.try_into()?);
            let framework_state=get_state_payload(pool, &framework_state_hash).await?;
            let app_state=get_state_payload(pool, &app_state_hash).await?;
            Ok(LatestState::Latest { last_event_height,last_state_height, framework_state, app_state })
        },
        None => Ok(LatestState::NoState),
    }
}

async fn get_state_payload(pool: &sqlx::SqlitePool, hash: &[u8]) -> Result<Vec<u8>> {
    Ok(
        sqlx::query_scalar!("SELECT payload FROM state_payload WHERE hash=$1", hash)
            .fetch_one(pool)
            .await?,
    )
}
