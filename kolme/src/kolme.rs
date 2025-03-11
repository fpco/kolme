use std::path::Path;

use anyhow::Context;
use sqlx::sqlite::SqliteConnectOptions;

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
    pub state: Arc<RwLock<KolmeState<App>>>,
    pub app: App,
}

impl<App: KolmeApp> Kolme<App> {
    pub async fn new(
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
            LatestState::NoState { last_event_height } => {
                FrameworkState::new(last_event_height, App::genesis_info())?
            }
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
            inner: Arc::new(KolmeInner {
                pool,
                framework_state: Arc::new(RwLock::new(framework_state)),
                app,
            }),
        })
    }

    pub fn get_next_event_height(&self) -> EventHeight {
        self.inner.framework_state.read().next_event_height
    }

    pub fn get_next_state_height(&self) -> EventHeight {
        self.inner.framework_state.read().next_state_height
    }

    pub(crate) fn get_next_account_nonce(&self, public_key: k256::PublicKey) -> AccountNonce {
        let guard = self.inner.framework_state.read();
        match guard.get_account_id(&public_key) {
            None => AccountNonce::start(),
            Some(account_id) => guard.get_next_nonce(account_id).unwrap(),
        }
    }
}

enum LatestState {
    NoState {
        last_event_height: Option<EventHeight>,
    },
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
    let last_event_height = sqlx::query_scalar!("SELECT MAX(height) FROM event_stream")
        .fetch_one(pool)
        .await?
        .map(|i| i.try_into().map(EventHeight))
        .transpose()?;
    match sqlx::query_as!(
        Helper,
        "SELECT height, framework_state_hash, app_state_hash FROM state_stream ORDER BY height DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await? {
        Some(Helper { height: last_state_height, framework_state_hash, app_state_hash }) => {
            let last_event_height = last_event_height.context("Saw a state height with no event height")?;
            let last_state_height = EventHeight(last_state_height.try_into()?);
            anyhow::ensure!(last_event_height >= last_state_height);
            let framework_state=get_state_payload(pool, &framework_state_hash).await?;
            let app_state=get_state_payload(pool, &app_state_hash).await?;
            Ok(LatestState::Latest { last_event_height,last_state_height, framework_state, app_state })
        },
        None => Ok(LatestState::NoState{ last_event_height  }),
    }
}

async fn get_state_payload(pool: &sqlx::SqlitePool, hash: &[u8]) -> Result<Vec<u8>> {
    Ok(
        sqlx::query_scalar!("SELECT payload FROM state_payload WHERE hash=$1", hash)
            .fetch_one(pool)
            .await?,
    )
}
