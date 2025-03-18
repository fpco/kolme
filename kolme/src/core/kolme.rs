use std::{
    ops::{Deref, DerefMut},
    path::Path,
};

use k256::{
    ecdsa::{signature::SignerMut, SigningKey},
    SecretKey,
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
    notify: tokio::sync::broadcast::Sender<Notification<App>>,
}

impl<App: KolmeApp> Clone for Kolme<App> {
    fn clone(&self) -> Self {
        Kolme {
            inner: self.inner.clone(),
            notify: self.notify.clone(),
        }
    }
}

impl<App: KolmeApp> Kolme<App> {
    pub async fn read(&self) -> KolmeRead<App> {
        KolmeRead(self.inner.clone().read_owned().await)
    }

    pub async fn write(&self) -> KolmeWrite<App> {
        KolmeWrite {
            inner: self.inner.clone().write_owned().await,
            broadcast: self.notify.clone(),
        }
    }

    /// Notify the system of a genesis contract instantiation.
    pub fn notify_genesis_instantiation(&self, chain: ExternalChain, contract: String) {
        self.notify
            .send(Notification::GenesisInstantiation { chain, contract })
            .ok();
    }

    /// Propose a new event for the processor to add to the chain.
    pub fn propose_transaction(&self, tx: SignedTransaction<App::Message>) -> Result<()> {
        self.notify
            .send(Notification::Broadcast { tx })
            .map_err(|_| {
                anyhow::anyhow!(
                    "Tried to propose an event, but no one is listening to our notifications"
                )
            })
            .map(|_| ())
    }

    /// Validate and append the given block.
    pub async fn add_block(&self, signed_block: &SignedBlock<App::Message>) -> Result<()> {
        let exec_results = self
            .read()
            .await
            .execute_messages(
                &signed_block
                    .0
                    .message
                    .as_inner()
                    .tx
                    .0
                    .message
                    .as_inner()
                    .messages,
                Some(signed_block.0.message.as_inner().loads.clone()),
            )
            .await?;
        let mut kolme = self.write().await.begin_db_transaction().await?;
        kolme.store_block(signed_block, exec_results).await?;
        kolme.commit().await?;
        Ok(())
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
pub struct KolmeWrite<App: KolmeApp> {
    inner: tokio::sync::OwnedRwLockWriteGuard<KolmeInner<App>>,
    broadcast: tokio::sync::broadcast::Sender<Notification<App>>,
}

impl<App: KolmeApp> Deref for KolmeWrite<App> {
    type Target = KolmeInner<App>;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<App: KolmeApp> DerefMut for KolmeWrite<App> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

pub struct KolmeInner<App: KolmeApp> {
    pub(super) pool: sqlx::SqlitePool,
    pub(super) app: App,
    pub(super) framework_state: FrameworkState,
    pub(super) app_state: App::State,
    pub(super) next_height: BlockHeight,
    pub(super) current_block_hash: BlockHash,
}

impl<App: KolmeApp> Kolme<App> {
    pub async fn new(
        app: App,
        _code_version: impl AsRef<str>,
        db_path: impl AsRef<Path>,
    ) -> Result<Self> {
        // FIXME in the future do some validation of code version, and allow
        // for explicit events for upgrading to a newer code version
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(options).await?;
        sqlx::migrate!().run(&pool).await?;
        let info = App::genesis_info();
        validate_genesis_info(&pool, &info).await?;
        let LoadStateResult {
            framework_state,
            app_state,
            next_height,
            current_block_hash,
        } = state::load_state::<App>(&pool, &info).await?;
        let inner = KolmeInner {
            pool,
            app,
            framework_state,
            app_state,
            next_height,
            current_block_hash,
        };
        Ok(Kolme {
            inner: Arc::new(tokio::sync::RwLock::new(inner)),
            notify: tokio::sync::broadcast::channel(100).0,
        })
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Notification<App>> {
        self.notify.subscribe()
    }
}

impl<App: KolmeApp> KolmeInner<App> {
    pub fn get_next_height(&self) -> BlockHeight {
        self.next_height
    }

    /// Returns the hash of the most recent block.
    ///
    /// If there is no event present, returns the special parent for the genesis block.
    pub fn get_current_block_hash(&self) -> BlockHash {
        self.current_block_hash
    }

    /// Get the ID of the next bridge event pending for the given chain.
    pub async fn get_next_bridge_event_id(
        &self,
        chain: ExternalChain,
        pubkey: PublicKey,
    ) -> Result<BridgeEventId> {
        let pubkey = pubkey.to_sec1_bytes();
        let chain = chain.as_ref();
        let bridge_event_id = sqlx::query_scalar!(
            r#"
                SELECT event_id
                FROM bridge_events
                LEFT JOIN bridge_event_attestations
                ON bridge_events.id=bridge_event_attestations.event
                WHERE chain=$1
                AND (public_key=$2 OR accepted IS NOT NULL)
                ORDER BY event_id DESC
                LIMIT 1
            "#,
            chain,
            pubkey
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match bridge_event_id {
            None => BridgeEventId::start(),
            Some(bridge_event_id) => BridgeEventId::try_from_i64(bridge_event_id)?.next(),
        })
    }

    pub fn get_next_genesis_action(&self) -> Option<GenesisAction> {
        for (chain, config) in &self.framework_state.chains {
            match config.bridge {
                BridgeContract::NeededCosmosBridge { code_id } => {
                    return Some(GenesisAction::InstantiateCosmos {
                        chain: *chain,
                        code_id,
                        processor: self.framework_state.processor,
                        listeners: self.framework_state.listeners.clone(),
                        needed_listeners: self.framework_state.needed_listeners,
                        executors: self.framework_state.executors.clone(),
                        needed_executors: self.framework_state.needed_executors,
                    })
                }
                BridgeContract::Deployed(_) => (),
            }
        }
        None
    }

    pub fn get_processor_pubkey(&self) -> PublicKey {
        self.framework_state.processor
    }

    pub fn get_bridge_contracts(&self) -> &BTreeMap<ExternalChain, ChainConfig> {
        &self.framework_state.chains
    }
}

// FIXME this code is pretty hairy, it would be nice to improve at some point in the future.
pub enum KolmeDbExecutor {
    Pool(sqlx::SqlitePool),
    Trans(std::sync::Arc<tokio::sync::Mutex<Option<sqlx::SqliteTransaction<'static>>>>),
}

/// Something which provides read access to a Kolme database.
pub trait KolmeDbReader {
    type App: KolmeApp;

    fn get_executor(&self) -> KolmeDbExecutor;

    /// Load the block details from the database
    #[allow(async_fn_in_trait)]
    async fn load_block(
        &self,
        height: BlockHeight,
    ) -> Result<SignedBlock<<Self::App as KolmeApp>::Message>> {
        let height = height.try_into_i64()?;
        let query = sqlx::query_scalar!(
            r#"
                SELECT rendered
                FROM blocks
                WHERE height=$1
                LIMIT 1
            "#,
            height
        );
        let payload = match self.get_executor() {
            KolmeDbExecutor::Pool(pool) => query.fetch_one(&pool).await,
            KolmeDbExecutor::Trans(mutex) => {
                query
                    .fetch_one(&mut **mutex.lock().await.as_mut().unwrap())
                    .await
            }
        }?;
        serde_json::from_str(&payload).map_err(anyhow::Error::from)
    }

    async fn get_next_account_nonce(&self, key: PublicKey) -> Result<AccountNonce> {
        let key = key.to_sec1_bytes();
        let query = sqlx::query_scalar!(
            r#"
                SELECT nonce
                FROM blocks
                INNER JOIN account_pubkeys
                ON account_pubkeys.account_id=blocks.account_id
                WHERE account_pubkeys.pubkey=$1
                ORDER BY nonce DESC
                LIMIT 1
            "#,
            key
        );
        let nonce = match self.get_executor() {
            KolmeDbExecutor::Pool(pool) => query.fetch_optional(&pool).await?,
            KolmeDbExecutor::Trans(mutex) => {
                query
                    .fetch_optional(&mut **mutex.lock().await.as_mut().unwrap())
                    .await?
            }
        };
        match nonce {
            Some(nonce) => Ok(AccountNonce::try_from(nonce)?.next()),
            None => Ok(AccountNonce::start()),
        }
    }
}

impl<App: KolmeApp> KolmeDbReader for KolmeRead<App> {
    type App = App;

    fn get_executor(&self) -> KolmeDbExecutor {
        KolmeDbExecutor::Pool(self.pool.clone())
    }
}

/// An upgraded [KolmeWrite] which also has an active DB transaction.
pub struct KolmeWriteDb<App: KolmeApp> {
    kolme: KolmeWrite<App>,
    trans: Arc<tokio::sync::Mutex<Option<sqlx::SqliteTransaction<'static>>>>,
}

impl<App: KolmeApp> KolmeWrite<App> {
    pub async fn begin_db_transaction(self) -> Result<KolmeWriteDb<App>> {
        let trans = self.pool.begin().await?;
        Ok(KolmeWriteDb {
            kolme: self,
            trans: Arc::new(tokio::sync::Mutex::new(Some(trans))),
        })
    }
}

impl<App: KolmeApp> KolmeWriteDb<App> {
    pub async fn commit(self) -> Result<KolmeWrite<App>> {
        match self
            .trans
            .lock()
            .await
            .take()
            .expect("Impossible None in KolmeWriteDb::commit")
            .commit()
            .await
        {
            Ok(()) => Ok(self.kolme),
            Err(e) => Err(e.into()),
        }
    }

    /// Also performs validation of the data
    async fn store_block(
        &mut self,
        signed_block: &SignedBlock<App::Message>,
        ExecutionResults {
            framework_state,
            app_state,
            outputs,
        }: ExecutionResults<App>,
    ) -> Result<()> {
        let block = signed_block.0.message.as_inner();
        let tx = block.tx.0.message.as_inner();

        let height = block.height;
        assert_eq!(self.get_next_height(), height);
        let height_i64 = height.try_into_i64()?;

        let (account_id, nonce) = self
            .get_or_insert_account_id_and_next_nonce(tx.pubkey, height)
            .await?;
        anyhow::ensure!(nonce == tx.nonce);
        let account_id = i64::try_from(account_id.0)?;
        let nonce = i64::try_from(nonce.0)?;

        let blockhash = signed_block.0.message_hash();
        let rendered = serde_json::to_string(&signed_block)?;
        let txhash = signed_block.0.message.as_inner().tx.0.message_hash();

        let framework_state_rendered = serde_json::to_string(&framework_state)?;
        let framework_state_hash = insert_state_payload(
            self.trans.lock().await.as_mut().unwrap(),
            &framework_state_rendered,
        )
        .await?;
        let app_state_rendered = App::save_state(&app_state)?;
        let app_state_hash = insert_state_payload(
            self.trans.lock().await.as_mut().unwrap(),
            &app_state_rendered,
        )
        .await?;

        sqlx::query!(
            r#"
                INSERT INTO
                blocks(height, blockhash, rendered, txhash, framework_state_hash, app_state_hash, account_id, nonce)
                VALUES($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            height_i64,
            blockhash,
            rendered,
            txhash,
            framework_state_hash,
            app_state_hash,
            account_id,
            nonce,
        ).execute(&mut **self.trans.lock().await.as_mut().unwrap()).await?;

        for (
            message,
            MessageOutput {
                logs,
                loads,
                actions,
            },
        ) in outputs.iter().enumerate()
        {
            let message = i64::try_from(message)?;
            let message = sqlx::query!(
                "INSERT INTO messages(height, message) VALUES($1, $2)",
                height_i64,
                message
            )
            .execute(&mut **self.trans.lock().await.as_mut().unwrap())
            .await?
            .last_insert_rowid();
            for (position, log) in logs.iter().enumerate() {
                let position = i64::try_from(position)?;
                sqlx::query!(
                    "INSERT INTO logs(message, position, payload) VALUES($1, $2, $3)",
                    message,
                    position,
                    log
                )
                .execute(&mut **self.trans.lock().await.as_mut().unwrap())
                .await?;
            }
            for (position, load) in loads.iter().enumerate() {
                let position = i64::try_from(position)?;
                let load = serde_json::to_string(&load)?;
                sqlx::query!(
                    "INSERT INTO loads(message, position, payload) VALUES($1, $2, $3)",
                    message,
                    position,
                    load
                )
                .execute(&mut **self.trans.lock().await.as_mut().unwrap())
                .await?;
            }
            for (position, action) in actions.iter().enumerate() {
                let position = i64::try_from(position)?;
                let action = serde_json::to_string(&action)?;
                sqlx::query!(
                    "INSERT INTO actions(message, position, payload) VALUES($1, $2, $3)",
                    message,
                    position,
                    action
                )
                .execute(&mut **self.trans.lock().await.as_mut().unwrap())
                .await?;
            }
        }

        Ok(())
    }

    // self.state.event.increment_height();
    // let account_id = self.get_or_insert_account_id(
    //     &signed_block
    //         .0
    //         .message
    //         .as_inner()
    //         .event
    //         .0
    //         .message
    //         .as_inner()
    //         .pubkey,
    //     height,
    // );
    // // TODO: review this code more carefully, do we need to check nonces more explicitly? Can we make a higher-level abstraction to avoid exposing too many internals to users of Kolme?
    // self.bump_nonce_for(account_id)?;
    // let now = Timestamp::now();

    // let height_i64 = height.try_into_i64()?;
    // let now = now.to_string();
    // self.add_event_to_combined(height_i64, now, &signed_block)
    //     .await?;

    // // We ignore errors from .send. These only occur when there are no receivers, which is a normal state in the framework (it means no components need updates).
    // self.kolme
    //     .broadcast
    //     .send(Notification::NewBlock(signed_block))
    //     .ok();
    // Ok(())
    // }

    // FIXME this function is probably the wrong level of abstraction
    async fn add_event_to_combined(
        &mut self,
        height: i64,
        now: String,
        signed_block: &SignedBlock<App::Message>,
    ) -> Result<()> {
        todo!()
        // let hash = signed_block.0.message_hash();
        // let signed_event_str = serde_json::to_string(signed_block)?;
        // let combined_id = sqlx::query!("INSERT INTO combined_stream(height,added,is_execution,rendered) VALUES($1,$2,FALSE,$3)", height, now, signed_event_str).execute(&mut **self.trans.lock().await.as_mut().unwrap()).await?.last_insert_rowid();
        // let event_state = self.kolme.state.event.serialize_raw_state()?;
        // let event_state = insert_state_payload(
        //     &mut *self.trans.lock().await.as_mut().unwrap(),
        //     &event_state,
        // )
        // .await?;
        // sqlx::query!(
        //     "INSERT INTO event_stream(height,hash,state,rendered_id) VALUES($1,$2,$3,$4)",
        //     height,
        //     hash,
        //     event_state,
        //     combined_id
        // )
        // .execute(&mut **self.trans.lock().await.as_mut().unwrap())
        // .await?;
        // Ok(())
    }

    async fn get_or_insert_account_id_and_next_nonce(
        &self,
        pubkey: PublicKey,
        height: BlockHeight,
    ) -> Result<(AccountId, AccountNonce)> {
        let mut guard = self.trans.lock().await;
        let trans = guard.as_mut().unwrap();
        let pubkey = pubkey.to_sec1_bytes();
        let account_id = sqlx::query_scalar!(
            "SELECT account_id FROM account_pubkeys WHERE pubkey=$1",
            pubkey
        )
        .fetch_optional(&mut **trans)
        .await?;
        match account_id {
            None => {
                let height = height.try_into_i64()?;
                let account_id = sqlx::query!("INSERT INTO accounts(created) VALUES($1)", height)
                    .execute(&mut **trans)
                    .await?
                    .last_insert_rowid();
                sqlx::query!(
                    "INSERT INTO account_pubkeys(account_id, pubkey) VALUES($1, $2)",
                    account_id,
                    pubkey
                )
                .execute(&mut **trans)
                .await?;
                Ok((AccountId(account_id.try_into()?), AccountNonce::start()))
            }
            Some(account_id) => {
                let nonce = sqlx::query_scalar!(
                    r#"
                        SELECT nonce
                        FROM blocks
                        WHERE account_id=$1
                        ORDER BY nonce DESC
                        LIMIT 1
                    "#,
                    account_id
                )
                .fetch_optional(&mut **trans)
                .await?;
                let nonce = match nonce {
                    Some(nonce) => AccountNonce(nonce.try_into()?),
                    None => AccountNonce::start(),
                };
                Ok((AccountId(account_id.try_into()?), nonce))
            }
        }
    }
}

impl<App: KolmeApp> KolmeDbReader for KolmeWriteDb<App> {
    type App = App;

    fn get_executor(&self) -> KolmeDbExecutor {
        KolmeDbExecutor::Trans(self.trans.clone())
    }
}

impl<App: KolmeApp> Deref for KolmeWriteDb<App> {
    type Target = KolmeInner<App>;

    fn deref(&self) -> &Self::Target {
        self.kolme.deref()
    }
}

impl<App: KolmeApp> DerefMut for KolmeWriteDb<App> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.kolme.deref_mut()
    }
}
