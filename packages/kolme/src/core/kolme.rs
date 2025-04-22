mod error;
mod mempool;

pub use error::KolmeError;

#[cfg(feature = "pass_through")]
use std::sync::OnceLock;
use std::{collections::HashMap, ops::Deref, path::Path};

use mempool::Mempool;
use sqlx::sqlite::SqliteConnectOptions;
use tokio::sync::broadcast::error::RecvError;

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
    notify: tokio::sync::broadcast::Sender<Notification<App::Message>>,
    mempool: Mempool<App::Message>,
}

impl<App: KolmeApp> Clone for Kolme<App> {
    fn clone(&self) -> Self {
        Kolme {
            inner: self.inner.clone(),
            notify: self.notify.clone(),
            mempool: self.mempool.clone(),
        }
    }
}

impl<App: KolmeApp> Kolme<App> {
    /// Lock the local storage and the database.
    ///
    /// Purpose: ensure that nothing else is able to modify the Kolme
    /// state and give us inconsistent results.
    ///
    /// Under the surface, this uses an [tokio::sync::RwLock], so
    /// multiple reads do not block each other.
    pub async fn read(&self) -> KolmeRead<App> {
        #[cfg(feature = "deadlock_detector")]
        {
            let backtrace = std::backtrace::Backtrace::force_capture();
            let id = {
                let guard = self.inner.read().await;
                let mut guard = guard.deadlock_detector.write().unwrap();
                let id = guard.next_read_id;
                guard.next_read_id += 1;
                guard.active_reads.insert(id, backtrace);
                id
            };
            KolmeRead {
                guard: self.inner.clone().read_owned().await,
                id,
            }
        }
        #[cfg(not(feature = "deadlock_detector"))]
        KolmeRead {
            guard: self.inner.clone().read_owned().await,
        }
    }

    /// Send a general purpose notification.
    pub fn notify(&self, note: Notification<App::Message>) {
        if let Notification::Broadcast { tx } = &note {
            self.mempool.add(tx.clone());
        }
        self.notify.send(note).ok();
    }

    /// Notify the system of a genesis contract instantiation.
    pub fn notify_genesis_instantiation(&self, chain: ExternalChain, contract: String) {
        self.notify
            .send(Notification::GenesisInstantiation { chain, contract })
            .ok();
    }

    /// Propose a new event for the processor to add to the chain.
    pub fn propose_transaction(&self, tx: SignedTransaction<App::Message>) -> Result<()> {
        let tx = Arc::new(tx);
        self.mempool.add(tx.clone());
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
    pub async fn add_block(&self, signed_block: SignedBlock<App::Message>) -> Result<()> {
        let txhash = signed_block.0.message.as_inner().tx.hash();
        signed_block.validate_signature()?;
        let block = signed_block.0.message.as_inner();
        let exec_results = self
            .read()
            .await
            .execute_transaction(
                &block.tx,
                block.timestamp,
                Some(signed_block.0.message.as_inner().loads.clone()),
            )
            .await?;
        #[cfg(feature = "deadlock_detector")]
        {
            for (id, backtrace) in &self
                .inner
                .read()
                .await
                .deadlock_detector
                .read()
                .unwrap()
                .active_reads
            {
                println!("{id}: {backtrace}");
            }
        }
        let mut kolme = self.inner.write().await;
        let mut trans = kolme.pool.begin().await?;
        store_block(&mut kolme, &mut trans, &signed_block, &exec_results).await?;

        // And try to write to durable storage here, allowing a failure to revert this commit.
        if let Some(block_db) = &kolme.block_db {
            block_db.add_block(&signed_block).await?;
        }

        trans.commit().await?;
        kolme.next_height = signed_block.0.message.as_inner().height.next();
        kolme.current_block_hash = signed_block.hash();
        kolme.framework_state = exec_results.framework_state;
        kolme.app_state = exec_results.app_state;
        self.mempool.drop_tx(txhash);
        self.notify
            .send(Notification::NewBlock(Arc::new(signed_block)))
            .ok();
        Ok(())
    }

    pub(crate) async fn set_block_db(&self, block_db: BlockDb) {
        self.inner.write().await.block_db = Some(block_db);
    }

    pub async fn wait_on_mempool(&self) -> Arc<SignedTransaction<App::Message>> {
        loop {
            let (txhash, tx) = self.mempool.peek().await;
            match self.read().await.get_tx(txhash).await {
                Ok(Some(_)) => self.mempool.drop_tx(txhash),
                Ok(None) => {
                    break tx;
                }
                Err(e) => {
                    tracing::warn!("Error checking for transaction in database: {e}");
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        }
    }
}

/// Read-only access to Kolme.
pub struct KolmeRead<App: KolmeApp> {
    guard: tokio::sync::OwnedRwLockReadGuard<KolmeInner<App>>,
    #[cfg(feature = "deadlock_detector")]
    id: usize,
}

#[cfg(feature = "deadlock_detector")]
impl<App: KolmeApp> Drop for KolmeRead<App> {
    fn drop(&mut self) {
        self.guard
            .deadlock_detector
            .write()
            .unwrap()
            .active_reads
            .remove(&self.id);
    }
}

impl<App: KolmeApp> Deref for KolmeRead<App> {
    type Target = KolmeInner<App>;

    fn deref(&self) -> &Self::Target {
        self.guard.deref()
    }
}

pub struct KolmeInner<App: KolmeApp> {
    pub(super) pool: sqlx::SqlitePool,
    pub(super) app: App,
    pub(super) framework_state: FrameworkState,
    pub(super) app_state: App::State,
    pub(super) next_height: BlockHeight,
    pub(super) current_block_hash: BlockHash,
    pub(super) cosmos_conns: tokio::sync::RwLock<HashMap<CosmosChain, cosmos::Cosmos>>,
    pub(super) solana_conns: tokio::sync::RwLock<HashMap<SolanaChain, Arc<SolanaClient>>>,
    #[cfg(feature = "pass_through")]
    pub(super) pass_through_conn: OnceLock<reqwest::Client>,
    #[cfg(feature = "deadlock_detector")]
    pub(super) deadlock_detector: std::sync::RwLock<DeadlockDetector>,
    pub(super) merkle_manager: MerkleManager,
    block_db: Option<BlockDb>,
}

#[cfg(feature = "deadlock_detector")]
#[derive(Default)]
pub(super) struct DeadlockDetector {
    pub(super) next_read_id: usize,
    pub(super) active_reads: HashMap<usize, std::backtrace::Backtrace>,
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
        let merkle_manager = MerkleManager::default();
        let LoadStateResult {
            framework_state,
            app_state,
            next_height,
            current_block_hash,
        } = state::load_state::<App>(&pool, &info, &merkle_manager).await?;
        let inner = KolmeInner {
            pool,
            app,
            framework_state,
            app_state,
            next_height,
            current_block_hash,
            cosmos_conns: tokio::sync::RwLock::new(HashMap::new()),
            solana_conns: tokio::sync::RwLock::new(HashMap::new()),
            #[cfg(feature = "pass_through")]
            pass_through_conn: OnceLock::new(),
            #[cfg(feature = "deadlock_detector")]
            deadlock_detector: Default::default(),
            merkle_manager,
            block_db: None,
        };
        Ok(Kolme {
            inner: Arc::new(tokio::sync::RwLock::new(inner)),
            notify: tokio::sync::broadcast::channel(100).0,
            mempool: Mempool::new(),
        })
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Notification<App::Message>> {
        self.notify.subscribe()
    }

    /// Wait until the given block is published
    pub async fn wait_for_block(
        &self,
        height: BlockHeight,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        // Start an outer loop so that we can keep processing if we end up Lagged
        loop {
            // First subscribe to avoid a race condition...
            let mut recv = self.subscribe();
            // And then check if we're at the requested height.
            if let Some(block) = self.read().await.get_block(height).await? {
                return Ok(Arc::new(block));
            }
            loop {
                match recv.recv().await {
                    Ok(note) => match note {
                        Notification::NewBlock(block) => {
                            if block.0.message.as_inner().height >= height {
                                return Ok(block);
                            }
                        }
                        Notification::GenesisInstantiation { .. } => (),
                        Notification::Broadcast { .. } => (),
                        Notification::FailedTransaction { .. } => (),
                    },
                    Err(e) => match e {
                        RecvError::Closed => panic!("wait_for_block: unexpected Closed"),
                        RecvError::Lagged(_) => break,
                    },
                }
            }
        }
    }

    /// Wait until the given transaction is published
    pub async fn wait_for_tx(&self, tx: TxHash) -> Result<Arc<SignedBlock<App::Message>>> {
        // Start an outer loop so that we can keep processing if we end up Lagged
        loop {
            // First subscribe to avoid a race condition...
            let mut recv = self.subscribe();
            // And then check if we have that transaction.
            if let Some(block) = self.read().await.get_tx(tx).await? {
                break Ok(Arc::new(block));
            }
            loop {
                match recv.recv().await {
                    Ok(note) => match note {
                        Notification::NewBlock(block) => {
                            if block.0.message.as_inner().tx.hash() == tx {
                                return Ok(block);
                            }
                        }
                        Notification::GenesisInstantiation { .. } => (),
                        Notification::Broadcast { .. } => (),
                        Notification::FailedTransaction { .. } => (),
                    },
                    Err(e) => match e {
                        RecvError::Closed => panic!("wait_for_tx: unexpected Closed"),
                        RecvError::Lagged(_) => break,
                    },
                }
            }
        }
    }
}

impl<App: KolmeApp> KolmeInner<App> {
    pub async fn get_cosmos(&self, chain: CosmosChain) -> Result<cosmos::Cosmos> {
        if let Some(cosmos) = self.cosmos_conns.read().await.get(&chain) {
            return Ok(cosmos.clone());
        }

        let mut guard = self.cosmos_conns.write().await;
        match guard.get(&chain) {
            Some(cosmos) => Ok(cosmos.clone()),
            None => {
                let cosmos = chain.make_client().await?;
                guard.insert(chain, cosmos.clone());
                Ok(cosmos)
            }
        }
    }

    pub async fn get_solana_client(&self, chain: SolanaChain) -> Arc<SolanaClient> {
        if let Some(client) = self.solana_conns.read().await.get(&chain) {
            return client.clone();
        }

        let mut guard = self.solana_conns.write().await;
        match guard.get(&chain) {
            Some(client) => Arc::clone(client),
            None => {
                let client = Arc::new(chain.make_client());
                guard.insert(chain, Arc::clone(&client));

                client
            }
        }
    }

    #[cfg(feature = "pass_through")]
    pub async fn get_pass_through_client(&self) -> reqwest::Client {
        self.pass_through_conn
            .get_or_init(reqwest::Client::new)
            .clone()
    }

    pub fn get_next_height(&self) -> BlockHeight {
        self.next_height
    }

    /// Returns the given block, if available.
    pub async fn get_block(
        &self,
        height: BlockHeight,
    ) -> Result<Option<SignedBlock<App::Message>>> {
        let height = i64::try_from(height.0)?;
        let rendered = sqlx::query_scalar!(
            r#"
                SELECT rendered
                FROM blocks
                WHERE height=$1
                LIMIT 1
            "#,
            height
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match rendered {
            Some(rendered) => serde_json::from_str(&rendered)?,
            None => None,
        })
    }

    /// Get the block information for the given transaction, if present.
    pub async fn get_tx(&self, tx: TxHash) -> Result<Option<SignedBlock<App::Message>>> {
        let tx = tx.0;
        let rendered = sqlx::query_scalar!(
            r#"
                SELECT rendered
                FROM blocks
                WHERE txhash=$1
                LIMIT 1
            "#,
            tx
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match rendered {
            Some(rendered) => serde_json::from_str(&rendered)?,
            None => None,
        })
    }

    /// Get the [MerkleManager]
    pub fn get_merkle_manager(&self) -> &MerkleManager {
        &self.merkle_manager
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
        for (chain, state) in self.framework_state.chains.iter() {
            match &state.config.bridge {
                BridgeContract::NeededCosmosBridge { code_id } => {
                    return Some(GenesisAction::InstantiateCosmos {
                        chain: chain.to_cosmos_chain().unwrap(),
                        code_id: *code_id,
                        args: self.framework_state.instantiate_args(),
                    })
                }
                BridgeContract::NeededSolanaBridge { program_id } => {
                    return Some(GenesisAction::InstantiateSolana {
                        chain: chain.to_solana_chain().unwrap(),
                        program_id: program_id.clone(),
                        args: self.framework_state.instantiate_args(),
                    })
                }
                BridgeContract::Deployed(_) => (),
            }
        }

        None
    }

    pub fn get_next_bridge_action(
        &self,
        chain: ExternalChain,
    ) -> Result<Option<(BridgeActionId, &PendingBridgeAction)>> {
        Ok(self
            .framework_state
            .chains
            .get(chain)?
            .pending_actions
            .iter()
            .next()
            .map(|(k, v)| (*k, v)))
    }

    pub fn get_app_state(&self) -> &App::State {
        &self.app_state
    }

    pub fn get_processor_pubkey(&self) -> PublicKey {
        self.framework_state.processor
    }

    pub fn get_approver_pubkeys(&self) -> &BTreeSet<PublicKey> {
        &self.framework_state.approvers
    }

    pub fn get_needed_approvers(&self) -> usize {
        self.framework_state.needed_approvers
    }

    pub fn get_bridge_contracts(&self) -> &ChainStates {
        &self.framework_state.chains
    }

    pub fn get_balances(&self) -> &Accounts {
        &self.framework_state.accounts
    }

    pub async fn create_signed_transaction(
        &self,
        secret: &SecretKey,
        messages: Vec<Message<App::Message>>,
    ) -> Result<SignedTransaction<App::Message>> {
        let pubkey = secret.public_key();
        let nonce = self.get_next_nonce(pubkey);
        let tx = Transaction::<App::Message> {
            pubkey,
            nonce,
            created: Timestamp::now(),
            messages,
        };
        tx.sign(secret)
    }

    /// Load the block details from the database
    pub async fn load_block(&self, height: BlockHeight) -> Result<SignedBlock<App::Message>> {
        let height = height.try_into_i64()?;
        let payload = sqlx::query_scalar!(
            r#"
                SELECT rendered
                FROM blocks
                WHERE height=$1
                LIMIT 1
            "#,
            height
        )
        .fetch_one(&self.pool)
        .await?;
        serde_json::from_str(&payload).map_err(anyhow::Error::from)
    }

    /// Get the next nonce to be used for the account associated with this public key.
    ///
    /// This function is read-only, and works for both accounts that do and don't exist.
    ///
    /// For new accounts, it will always return the initial nonce.
    pub fn get_next_nonce(&self, key: PublicKey) -> AccountNonce {
        self.framework_state
            .accounts
            .get_account_for_key(key)
            .map_or_else(AccountNonce::start, |(_, account)| account.get_next_nonce())
    }

    /// Get the account ID and next nonce for the given public key.
    ///
    /// This will insert a new account into the accounts map if needed.
    pub fn get_account_and_next_nonce(&mut self, key: PublicKey) -> AccountAndNextNonce {
        let (id, account) = self
            .framework_state
            .accounts
            .get_or_add_account_for_pubkey(key);
        AccountAndNextNonce {
            id,
            next_nonce: account.get_next_nonce(),
        }
    }

    pub async fn received_listener_attestation(
        &self,
        chain: ExternalChain,
        pubkey: PublicKey,
        event_id: BridgeEventId,
    ) -> Result<bool> {
        let chain = chain.as_ref();
        let event_id = i64::try_from(event_id.0)?;

        let count = sqlx::query_scalar!(
            r#"
                SELECT COUNT(*)
                FROM bridge_events
                INNER JOIN bridge_event_attestations
                ON bridge_events.id=bridge_event_attestations.event
                WHERE bridge_events.chain=$1
                AND bridge_events.event_id=$2
                AND public_key=$3
            "#,
            chain,
            event_id,
            pubkey
        )
        .fetch_one(&self.pool)
        .await?;
        assert!(count == 0 || count == 1);
        Ok(count == 1)
    }
}

/// Response from [get_account_and_next_nonce]
pub struct AccountAndNextNonce {
    /// The account ID
    pub id: AccountId,
    /// The next nonce to be used
    pub next_nonce: AccountNonce,
}

/// Also performs validation of the data
async fn store_block<App: KolmeApp>(
    kolme: &mut KolmeInner<App>,
    trans: &mut sqlx::SqliteTransaction<'_>,
    signed_block: &SignedBlock<App::Message>,
    ExecutionResults {
        framework_state,
        app_state,
        db_updates,
        logs,
        loads,
    }: &ExecutionResults<App>,
) -> Result<()> {
    let block = signed_block.0.message.as_inner();
    let tx = block.tx.0.message.as_inner();

    anyhow::ensure!(loads == &block.loads);

    let height = block.height;
    let expected = kolme.get_next_height();
    if height != expected {
        return Err(anyhow::Error::from(KolmeError::InvalidAddBlockHeight {
            proposed: height,
            expected,
        }));
    }
    let height_i64 = height.try_into_i64()?;

    let blockhash = signed_block.0.message_hash();
    let rendered = serde_json::to_string(&signed_block)?;
    let txhash = signed_block.0.message.as_inner().tx.0.message_hash();

    let mut store = MerkleDbStore::Conn(trans);
    let framework_state_hash = kolme
        .merkle_manager
        .save(&mut store, framework_state)
        .await?
        .hash;
    let app_state_hash = kolme.merkle_manager.save(&mut store, app_state).await?.hash;

    sqlx::query!(
        r#"
            INSERT INTO
            blocks(height, blockhash, rendered, txhash, framework_state_hash, app_state_hash)
            VALUES($1, $2, $3, $4, $5, $6)
        "#,
        height_i64,
        blockhash,
        rendered,
        txhash,
        framework_state_hash,
        app_state_hash,
    )
    .execute(&mut **trans)
    .await?;

    let mut message_db_ids = vec![];

    for (message, logs) in logs.iter().enumerate() {
        let message = i64::try_from(message)?;
        let message = sqlx::query!(
            "INSERT INTO messages(height, message) VALUES($1, $2)",
            height_i64,
            message
        )
        .execute(&mut **trans)
        .await?
        .last_insert_rowid();
        message_db_ids.push(message);
        for (position, log) in logs.iter().enumerate() {
            let position = i64::try_from(position)?;
            sqlx::query!(
                "INSERT INTO logs(message, position, payload) VALUES($1, $2, $3)",
                message,
                position,
                log
            )
            .execute(&mut **trans)
            .await?;
        }
    }

    for update in db_updates {
        match update {
            DatabaseUpdate::ListenerAttestation {
                chain,
                event_id,
                event_content,
                msg_index,
                was_accepted,
                action_id,
            } => {
                let message_db_id = message_db_ids[*msg_index];
                let chain = chain.as_ref();
                let event_id = i64::try_from(event_id.0)?;

                let event_db_id = sqlx::query_scalar!(
                    r#"
                        SELECT id
                        FROM bridge_events
                        WHERE chain=$1
                        AND event_id=$2
                    "#,
                    chain,
                    event_id
                )
                .fetch_optional(&mut **trans)
                .await?;
                let event_db_id = match event_db_id {
                    Some(id) => id,
                    None => sqlx::query!(
                        r#"
                            INSERT INTO
                            bridge_events(chain, event_id, event)
                            VALUES($1, $2, $3)
                        "#,
                        chain,
                        event_id,
                        event_content
                    )
                    .execute(&mut **trans)
                    .await?
                    .last_insert_rowid(),
                };

                sqlx::query!(
                    r#"
                        INSERT INTO
                        bridge_event_attestations(event, public_key, message)
                        VALUES($1, $2, $3)
                    "#,
                    event_db_id,
                    tx.pubkey,
                    message_db_id,
                )
                .execute(&mut **trans)
                .await?;

                if *was_accepted {
                    let rows = sqlx::query!(
                        r#"
                            UPDATE bridge_events
                            SET accepted=$1
                            WHERE chain=$2
                            AND event_id=$3
                        "#,
                        message_db_id,
                        chain,
                        event_id,
                    )
                    .execute(&mut **trans)
                    .await?
                    .rows_affected();
                    anyhow::ensure!(rows == 1);

                    if let Some(action_id) = *action_id {
                        todo!("Need to log completion of the action: {action_id}");
                    }
                }
            }
        }
    }

    Ok(())
}
