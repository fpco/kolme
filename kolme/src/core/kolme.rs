use std::{
    ops::{Deref, DerefMut},
    path::Path,
};

use sqlx::sqlite::SqliteConnectOptions;

use crate::core::*;

/// A running instance of Kolme for the given application.
///
/// This type represents the core execution environment for Kolme.
/// It handles data storage, queries, subscriptions, etc.
///
/// This value can be cloned cheaply, allowing the same Kolme storage
/// to be shared across multiple different components in a single
/// executable.
///
/// All operations must start with either getting a read or write lock.
/// This ensures both database and in-memory storage are appropriately
/// locked during write operations to prevent data races.
pub struct Kolme<App: KolmeApp> {
    inner: Arc<tokio::sync::RwLock<KolmeInner<App>>>,
}

impl<App: KolmeApp> Clone for Kolme<App> {
    fn clone(&self) -> Self {
        Kolme {
            inner: self.inner.clone(),
        }
    }
}

impl<App: KolmeApp> Kolme<App> {
    pub async fn read(&self) -> KolmeRead<App> {
        KolmeRead(self.inner.clone().read_owned().await)
    }

    pub async fn write(&self) -> KolmeWrite<App> {
        KolmeWrite(self.inner.clone().write_owned().await)
    }
}

/// Read-only access to Kolme.
pub struct KolmeRead<App: KolmeApp>(tokio::sync::OwnedRwLockReadGuard<KolmeInner<App>>);

impl<App: KolmeApp> Deref for KolmeRead<App> {
    type Target = KolmeInner<App>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

/// Read/write access to Kolme.
pub struct KolmeWrite<App: KolmeApp>(tokio::sync::OwnedRwLockWriteGuard<KolmeInner<App>>);

impl<App: KolmeApp> Deref for KolmeWrite<App> {
    type Target = KolmeInner<App>;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<App: KolmeApp> DerefMut for KolmeWrite<App> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

pub(super) struct KolmeInner<App: KolmeApp> {
    pub(super) pool: sqlx::SqlitePool,
    pub(super) state: KolmeState<App>,
    pub(super) app: App,
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
        let state = KolmeState::new(&app, &pool, code_version).await?;
        Ok(Kolme {
            inner: Arc::new(tokio::sync::RwLock::new(KolmeInner { pool, state, app })),
        })
    }
}

impl<App: KolmeApp> KolmeInner<App> {
    pub fn get_next_event_height(&self) -> EventHeight {
        self.state.event.get_next_height()
    }

    pub fn get_next_exec_height(&self) -> EventHeight {
        self.state.exec.get_next_height()
    }

    pub fn get_next_account_nonce(&self, key: PublicKey) -> AccountNonce {
        self.state.event.get_next_account_nonce(key)
    }

    /// Load the event details from the database
    pub async fn load_event(&self, height: EventHeight) -> Result<SignedEvent<App::Message>> {
        let height = height.try_into_i64()?;
        let query = sqlx::query_scalar!(
            r#"
            SELECT rendered
            FROM combined_stream
            WHERE height=$1
            AND NOT is_execution
            LIMIT 1
        "#,
            height
        );
        let payload = query.fetch_one(&self.pool).await?;
        serde_json::from_str(&payload).map_err(anyhow::Error::from)
    }
}
