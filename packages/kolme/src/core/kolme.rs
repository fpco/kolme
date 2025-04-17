mod error;
mod mempool;

pub use error::KolmeError;

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
        }
        KolmeRead {
            guard: self.inner.clone().read_owned().await,
            #[cfg(feature = "deadlock_detector")]
            id,
        }
    }

    /// Send a general purpose notification.
    pub fn notify(&self, note: Notification<App::Message>) {
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
    #[cfg(feature = "deadlock_detector")]
    pub(super) deadlock_detector: std::sync::RwLock<DeadlockDetector>,
    pub(super) merkle_manager: MerkleManager,
    block_db: Option<BlockDb>,
}

#[cfg(feature = "deadlock_detector")]
#[derive(Default)]
pub(super) struct DeadlockDetector {
    pub(super) next_read_id: usize,
    pub(super) active_reads: HashMap<usize, Backtrace>,
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
        for (chain, config) in &self.framework_state.chains.0 {
            match &config.bridge {
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

    pub async fn get_next_bridge_action(
        &self,
        chain: ExternalChain,
    ) -> Result<Option<PendingBridgeAction>> {
        struct Helper {
            height: i64,
            message: i64,
            payload: String,
            action_id: i64,
        }
        let chain_str = chain.as_ref();
        let helper = sqlx::query_as!(
            Helper,
            r#"
                SELECT messages.height, messages.message, actions.payload, actions.action_id
                FROM actions
                INNER JOIN messages
                ON actions.approved=messages.id
                WHERE chain=$1
                AND confirmed IS NULL
            "#,
            chain_str
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some(Helper {
            payload,
            height,
            message,
            action_id,
        }) = helper
        else {
            return Ok(None);
        };
        Ok(Some(PendingBridgeAction {
            chain,
            payload,
            height: BlockHeight::try_from(height)?,
            message: message.try_into()?,
            action_id: BridgeActionId(action_id.try_into()?),
        }))
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

    pub fn get_bridge_contracts(&self) -> &BTreeMap<ExternalChain, ChainConfig> {
        &self.framework_state.chains.0
    }

    pub fn get_balances(&self) -> &Balances {
        &self.framework_state.balances
    }

    pub async fn create_signed_transaction(
        &self,
        secret: &SecretKey,
        messages: Vec<Message<App::Message>>,
    ) -> Result<SignedTransaction<App::Message>> {
        let pubkey = secret.public_key();
        let nonce = self.get_account_and_next_nonce(pubkey).await?.next_nonce;
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

    /// Get the next available account ID in the database.
    pub async fn get_next_account_id(&self) -> Result<AccountId> {
        match sqlx::query_scalar!("SELECT id FROM accounts ORDER BY id DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await?
        {
            Some(id) => Ok(AccountId(id.try_into()?).next()),
            None => Ok(AccountId(1)),
        }
    }

    pub async fn get_account_and_next_nonce(&self, key: PublicKey) -> Result<AccountAndNextNonce> {
        let account_id = sqlx::query_scalar!(
            "SELECT account_id FROM account_pubkeys WHERE pubkey=$1",
            key
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(account_id) = account_id else {
            return Ok(AccountAndNextNonce {
                id: self.get_next_account_id().await?,
                exists: false,
                next_nonce: AccountNonce::start(),
            });
        };

        let last_nonce = sqlx::query_scalar!(
            r#"
                SELECT nonce
                FROM blocks
                WHERE account_id=$1
                ORDER BY nonce DESC
                LIMIT 1
            "#,
            account_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(AccountAndNextNonce {
            id: AccountId(account_id.try_into()?),
            exists: true,
            next_nonce: match last_nonce {
                Some(last_nonce) => AccountNonce(last_nonce.try_into()?).next(),
                None => AccountNonce::start(),
            },
        })
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

    /// Get the ID of the latest action emitted for the given chain, if present.
    pub async fn get_latest_action(&self, chain: ExternalChain) -> Result<Option<BridgeActionId>> {
        let chain = chain.as_ref();
        let id = sqlx::query_scalar!(
            r#"
                SELECT action_id
                FROM actions
                WHERE chain=$1
                ORDER BY action_id DESC
                LIMIT 1
            "#,
            chain
        )
        .fetch_optional(&self.pool)
        .await?;
        match id {
            Some(id) => Ok(Some(BridgeActionId(id.try_into()?))),
            None => Ok(None),
        }
    }

    /// Get the ID of the latest action on the given chain signed by the given key.
    pub async fn get_latest_approval(
        &self,
        chain: ExternalChain,
        pubkey: PublicKey,
    ) -> Result<Option<BridgeActionId>> {
        let chain = chain.as_ref();
        let latest_action_id = sqlx::query_scalar!(
            r#"
                SELECT actions.action_id
                FROM actions
                INNER JOIN action_approvals
                ON actions.id=action_approvals.action
                WHERE chain=$1
                AND public_key=$2
            "#,
            chain,
            pubkey,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match latest_action_id {
            Some(id) => Some(BridgeActionId(id.try_into()?).next()),
            None => None,
        })
    }

    /// Get the ID of the first action which has not yet been approved by the processor.
    pub async fn get_first_unapproved_action(
        &self,
        chain: ExternalChain,
    ) -> Result<Option<BridgeActionId>> {
        let chain = chain.as_ref();
        let id = sqlx::query_scalar!(
            r#"
                SELECT action_id
                FROM actions
                WHERE chain=$1
                AND approved IS NULL
                ORDER BY action_id ASC
                LIMIT 1
            "#,
            chain
        )
        .fetch_optional(&self.pool)
        .await?;
        match id {
            Some(id) => Ok(Some(BridgeActionId(id.try_into()?))),
            None => Ok(None),
        }
    }

    /// Get the public keys of all approver approvals on an action.
    pub async fn get_action_approval_signatures(
        &self,
        chain: ExternalChain,
        action_id: BridgeActionId,
    ) -> Result<BTreeMap<PublicKey, SignatureWithRecovery>> {
        struct Helper {
            public_key: Vec<u8>,
            signature: Vec<u8>,
            recovery: i64,
        }
        let chain = chain.as_ref();
        let action_id = i64::try_from(action_id.0)?;
        let helpers = sqlx::query_as!(
            Helper,
            r#"
                SELECT public_key, signature, recovery
                FROM actions
                INNER JOIN action_approvals
                ON actions.id=action_approvals.action
                WHERE chain=$1
                AND action_id=$2
            "#,
            chain,
            action_id,
        )
        .fetch_all(&self.pool)
        .await?;
        helpers
            .into_iter()
            .map(
                |Helper {
                     public_key,
                     signature,
                     recovery,
                 }| {
                    Ok((
                        PublicKey::try_from_bytes(&public_key)?,
                        SignatureWithRecovery {
                            sig: Signature::from_slice(&signature)?,
                            recid: RecoveryId::from_byte(recovery.try_into()?)
                                .context("Invalid recovery found")?,
                        },
                    ))
                },
            )
            .collect()
    }

    /// Get the payload of a bridge action.
    pub async fn get_action_payload(
        &self,
        chain: ExternalChain,
        action_id: BridgeActionId,
    ) -> Result<Vec<u8>> {
        get_action_payload(&self.pool, chain, action_id).await
    }
}

pub(super) async fn get_action_payload(
    pool: &sqlx::SqlitePool,
    chain: ExternalChain,
    action_id: BridgeActionId,
) -> Result<Vec<u8>> {
    use base64::Engine;

    let chain_str = chain.as_ref();
    let action_id = i64::try_from(action_id.0)?;
    let payload = sqlx::query_scalar!(
        r#"
                SELECT payload
                FROM actions
                WHERE chain=$1
                AND action_id=$2
            "#,
        chain_str,
        action_id
    )
    .fetch_one(pool)
    .await?;

    // TODO: This is a hack... we should probably be storing binary blobs in the DB instead of TEXT.
    match ChainKind::from(chain) {
        ChainKind::Cosmos(_) => Ok(payload.into_bytes()),
        ChainKind::Solana(_) => {
            let payload = base64::engine::general_purpose::STANDARD.decode(&payload)?;

            Ok(payload)
        }
    }
}

/// Response from [get_account_and_next_nonce]
pub struct AccountAndNextNonce {
    /// The account ID
    pub id: AccountId,
    /// Is this already in the database?
    pub exists: bool,
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
        outputs,
        db_updates,
    }: &ExecutionResults<App>,
) -> Result<()> {
    let block = signed_block.0.message.as_inner();
    let tx = block.tx.0.message.as_inner();

    let height = block.height;
    let expected = kolme.get_next_height();
    if height != expected {
        return Err(anyhow::Error::from(KolmeError::InvalidAddBlockHeight {
            proposed: height,
            expected,
        }));
    }
    let height_i64 = height.try_into_i64()?;

    let (account_id, nonce) =
        get_or_insert_account_id_and_next_nonce(trans, tx.pubkey, height).await?;
    anyhow::ensure!(nonce == tx.nonce, "Tried to store block, but expected nonce {nonce} didn't match actual nonce {}. Signed block:\n{}\nTransaction:\n{}", tx.nonce, signed_block.0.message.as_str(), serde_json::to_string_pretty(tx)?);
    let account_id = i64::try_from(account_id.0)?;
    let nonce = i64::try_from(nonce.0)?;

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
        ).execute(&mut **trans).await?;

    let mut message_db_ids = vec![];

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
        for (position, load) in loads.iter().enumerate() {
            let position = i64::try_from(position)?;
            let load = serde_json::to_string(&load)?;
            sqlx::query!(
                "INSERT INTO loads(message, position, payload) VALUES($1, $2, $3)",
                message,
                position,
                load
            )
            .execute(&mut **trans)
            .await?;
        }
        for (position, action) in actions.iter().enumerate() {
            let position = i64::try_from(position)?;
            let chain = match action {
                ExecAction::Transfer {
                    chain,
                    recipient: _,
                    funds: _,
                } => *chain,
            };
            let chain_str = chain.as_ref();

            let latest_action_id = sqlx::query_scalar!(
                "SELECT action_id FROM actions WHERE chain=$1 ORDER BY action_id DESC LIMIT 1",
                chain_str
            )
            .fetch_optional(&mut **trans)
            .await?;
            let action_id = latest_action_id.map_or(0, |x| x + 1);

            let payload = action.to_payload(
                chain,
                kolme.get_bridge_contracts(),
                BridgeActionId(action_id.try_into()?),
            )?;

            sqlx::query!(
                r#"
                    INSERT INTO actions(chain, action_id, message, position, payload)
                    VALUES($1, $2, $3, $4, $5)
                "#,
                chain_str,
                action_id,
                message,
                position,
                payload,
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
            DatabaseUpdate::AddAccount { id } => {
                let id = i64::try_from(id.0)?;
                sqlx::query!(
                    "INSERT INTO accounts(id, created) VALUES($1, $2)",
                    id,
                    height_i64
                )
                .execute(&mut **trans)
                .await?;
            }
            DatabaseUpdate::AddWalletToAccount { id, wallet } => {
                let id = i64::try_from(id.0)?;
                sqlx::query!(
                    "INSERT INTO account_wallets(account_id,wallet) VALUES($1, $2)",
                    id,
                    wallet
                )
                .execute(&mut **trans)
                .await?;
            }
            DatabaseUpdate::RemoveWalletFromAccount { id, wallet } => {
                let id = i64::try_from(id.0)?;
                let rows = sqlx::query!(
                    "DELETE FROM account_wallets WHERE account_id=$1 AND wallet=$2",
                    id,
                    wallet,
                )
                .execute(&mut **trans)
                .await?
                .rows_affected();
                anyhow::ensure!(rows == 1);
            }
            DatabaseUpdate::AddPubkeyToAccount {
                id,
                pubkey,
                ignore_errors,
            } => {
                let id = i64::try_from(id.0)?;
                if *ignore_errors {
                    sqlx::query!(
                        "INSERT OR IGNORE INTO account_pubkeys(account_id,pubkey) VALUES($1, $2)",
                        id,
                        pubkey
                    )
                } else {
                    sqlx::query!(
                        "INSERT INTO account_pubkeys(account_id,pubkey) VALUES($1, $2)",
                        id,
                        pubkey
                    )
                }
                .execute(&mut **trans)
                .await
                .with_context(|| {
                    format!("Error while adding pubkey from DatabaseUpdate: {id}/{pubkey}")
                })?;
            }
            DatabaseUpdate::RemovePubkeyFromAccount { id, key } => {
                let id = i64::try_from(id.0)?;
                let rows = sqlx::query!(
                    "DELETE FROM account_pubkeys WHERE account_id=$1 AND pubkey=$2",
                    id,
                    key,
                )
                .execute(&mut **trans)
                .await?
                .rows_affected();
                anyhow::ensure!(rows == 1);
            }
            DatabaseUpdate::ApproveAction {
                pubkey,
                signature,
                recovery,
                msg_index,
                chain,
                action_id,
            } => {
                let chain = chain.as_ref();
                let action_id = i64::try_from(action_id.0)?;
                let action = sqlx::query_scalar!(
                    r#"
                        SELECT id
                        FROM actions
                        WHERE chain=$1
                        AND action_id=$2
                    "#,
                    chain,
                    action_id,
                )
                .fetch_one(&mut **trans)
                .await?;

                let message_db_id = message_db_ids[*msg_index];
                let signature = signature.to_bytes();
                let signature = signature.as_slice();
                let recovery = recovery.to_byte();
                sqlx::query!(
                    r#"
                        INSERT INTO action_approvals
                        (action, public_key, signature, recovery, message)
                        VALUES($1, $2, $3, $4, $5)
                    "#,
                    action,
                    pubkey,
                    signature,
                    recovery,
                    message_db_id
                )
                .execute(&mut **trans)
                .await?;
            }
            DatabaseUpdate::ProcessorApproveAction {
                msg_index,
                chain,
                action_id,
            } => {
                let chain = chain.as_ref();
                let action_id = i64::try_from(action_id.0)?;
                let message_db_id = message_db_ids[*msg_index];

                let rows = sqlx::query!(
                    r#"
                        UPDATE actions
                        SET approved=$1
                        WHERE chain=$2
                        AND action_id=$3
                    "#,
                    message_db_id,
                    chain,
                    action_id
                )
                .execute(&mut **trans)
                .await?
                .rows_affected();
                anyhow::ensure!(rows == 1);
            }
        }
    }

    Ok(())
}

async fn get_or_insert_account_id_and_next_nonce(
    trans: &mut sqlx::SqliteTransaction<'_>,
    pubkey: PublicKey,
    height: BlockHeight,
) -> Result<(AccountId, AccountNonce)> {
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
            .await
            .with_context(|| format!("Error while adding pubkey {pubkey}"))?;
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
                Some(nonce) => AccountNonce(nonce.try_into()?).next(),
                None => AccountNonce::start(),
            };
            Ok((AccountId(account_id.try_into()?), nonce))
        }
    }
}
