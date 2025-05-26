mod block_info;
mod mempool;
mod store;

use block_info::BlockState;
pub(super) use block_info::{BlockInfo, MaybeBlockInfo};
use kolme_store::{KolmeStoreError, StorableBlock};
use parking_lot::RwLock;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use store::KolmeConstructLock;
pub use store::KolmeStore;

#[cfg(feature = "pass_through")]
use std::sync::OnceLock;
use std::{collections::HashMap, ops::Deref};

use mempool::Mempool;
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
/// This data structure uses shared storage. To avoid potential
/// inconsistent data from different block heights, any operations
/// that read block-specific data must first clone a copy of that
/// data by calling the [Kolme::read] method. This clone is a simple
/// [Arc] clone, and so is very cheap.
pub struct Kolme<App: KolmeApp> {
    inner: Arc<KolmeInner<App>>,
    tx_await_duration: tokio::time::Duration,
}

pub(super) struct KolmeInner<App: KolmeApp> {
    notify: tokio::sync::broadcast::Sender<Notification<App::Message>>,
    mempool: Mempool<App::Message>,
    pub(super) store: store::KolmeStore<App>,
    pub(super) app: App,
    pub(super) cosmos_conns: tokio::sync::RwLock<HashMap<CosmosChain, cosmos::Cosmos>>,
    pub(super) solana_conns: tokio::sync::RwLock<HashMap<SolanaChain, Arc<SolanaClient>>>,
    pub(super) solana_endpoints: parking_lot::RwLock<SolanaEndpoints>,
    #[cfg(feature = "pass_through")]
    pub(super) pass_through_conn: OnceLock<reqwest::Client>,
    pub(super) merkle_manager: MerkleManager,
    current_block: RwLock<Arc<MaybeBlockInfo<App>>>,
}

/// Access to a specific block height.
pub struct KolmeRead<App: KolmeApp> {
    kolme: Kolme<App>,
    current: Arc<MaybeBlockInfo<App>>,
}

impl<App: KolmeApp> Deref for KolmeRead<App> {
    type Target = Kolme<App>;

    fn deref(&self) -> &Self::Target {
        &self.kolme
    }
}

impl<App: KolmeApp> Clone for Kolme<App> {
    fn clone(&self) -> Self {
        Kolme {
            inner: self.inner.clone(),
            tx_await_duration: self.tx_await_duration,
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
    pub fn read(&self) -> KolmeRead<App> {
        KolmeRead {
            kolme: self.clone(),
            current: self.inner.current_block.read().clone(),
        }
    }

    /// Send a general purpose notification.
    pub fn notify(&self, note: Notification<App::Message>) {
        if let Notification::FailedTransaction(ref failed) = note {
            self.remove_from_mempool(failed.message.as_inner().txhash);
        }
        // Ignore errors from notifications, it just means no one
        // is subscribed.
        self.inner.notify.send(note).ok();
    }

    /// Propose a new transaction for the processor to add to the chain.
    ///
    /// Note that this will not detect any issues if the transaction is rejected.
    pub fn propose_transaction(&self, tx: Arc<SignedTransaction<App::Message>>) {
        self.inner.mempool.add(tx);
    }

    /// How long should we wait for a transaction to land before giving up?
    ///
    /// This affects [Self::propose_and_await_transaction] and [Self::sign_propose_await_transaction].
    ///
    /// Default: 10 seconds
    pub fn set_tx_await_duration(mut self, duration: tokio::time::Duration) -> Self {
        self.tx_await_duration = duration;
        self
    }

    /// Propose a new transaction and wait for it to land on chain.
    ///
    /// This can be useful for detecting when a transaction was rejected after proposing.
    pub async fn propose_and_await_transaction(
        &self,
        tx: Arc<SignedTransaction<App::Message>>,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        let txhash = tx.hash();
        match tokio::time::timeout(
            self.tx_await_duration,
            self.propose_and_await_transaction_inner(tx),
        )
        .await
        {
            Ok(res) => res,
            Err(e) => Err(anyhow::Error::from(e).context(format!(
                "Timed out proposing and awaiting transaction {txhash}"
            ))),
        }
    }

    async fn propose_and_await_transaction_inner(
        &self,
        tx: Arc<SignedTransaction<App::Message>>,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        let mut recv = self.subscribe();
        let txhash_orig = tx.hash();
        self.propose_transaction(tx);
        loop {
            let note = recv.recv().await?;
            match note {
                Notification::NewBlock(block) => {
                    if block.tx().hash() == txhash_orig {
                        break Ok(block);
                    }
                }
                Notification::GenesisInstantiation { .. } => (),
                Notification::FailedTransaction(failed) => {
                    let pubkey = match failed.verify_signature() {
                        Ok(pubkey) => pubkey,
                        Err(e) => {
                            tracing::warn!(
                            "Received invalid signature on a FailedTransaction notification: {e}"
                        );
                            continue;
                        }
                    };
                    if pubkey
                        != self
                            .read()
                            .get_framework_state()
                            .get_validator_set()
                            .processor
                    {
                        tracing::warn!("Received a FailedTransaction notification from {pubkey}, which is not the processor, ignoring");
                        continue;
                    }
                    let FailedTransaction { txhash, error } = failed.message.into_inner();
                    if txhash_orig == txhash {
                        break Err(error.into());
                    }
                }
            }

            // Just in case we jumped some blocks, check if it landed in the interim.
            if let Some(height) = self.get_tx_height(txhash_orig).await? {
                return self.wait_for_block(height).await;
            }
        }
    }

    /// Signed and propose a transaction.
    ///
    /// Automatically resigns with a new nonce if necessary.
    pub async fn sign_propose_await_transaction(
        &self,
        secret: &SecretKey,
        messages: Vec<Message<App::Message>>,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        match tokio::time::timeout(
            self.tx_await_duration,
            self.sign_propose_await_transaction_inner(secret, messages),
        )
        .await
        {
            Ok(res) => res,
            Err(e) => Err(anyhow::Error::from(e)
                .context("Timed out while signing/proposing/awaiting a transaction")),
        }
    }

    async fn sign_propose_await_transaction_inner(
        &self,
        secret: &SecretKey,
        messages: Vec<Message<App::Message>>,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        loop {
            let tx = Arc::new(
                self.read()
                    .create_signed_transaction(secret, messages.clone())?,
            );
            match self.propose_and_await_transaction_inner(tx).await {
                Ok(block) => break Ok(block),
                Err(e) => {
                    if let Some(KolmeError::InvalidNonce {
                        pubkey: _,
                        account_id: _,
                        expected,
                        actual,
                    }) = e.downcast_ref()
                    {
                        if actual < expected {
                            tracing::warn!("Retrying with new nonce: {e}");
                            continue;
                        }
                    }
                    break Err(e);
                }
            }
        }
    }

    /// Resync with the database.
    pub async fn resync(&self) -> Result<()> {
        loop {
            let next = self.inner.current_block.read().get_next_height();
            let Some(block) = self
                .inner
                .store
                .load_signed_block(self.get_merkle_manager(), next)
                .await?
            else {
                break Ok(());
            };
            self.add_block(block).await?;
        }
    }

    /// Validate and append the given block.
    pub async fn add_block(&self, signed_block: Arc<SignedBlock<App::Message>>) -> Result<()> {
        self.add_block_with(signed_block, DataLoadValidation::ValidateDataLoads)
            .await
    }

    pub(crate) async fn add_block_with(
        &self,
        signed_block: Arc<SignedBlock<App::Message>>,
        data_load_validation: DataLoadValidation,
    ) -> Result<()> {
        // Make sure we're at the right height for this and the correct processor is signing this.
        let kolme = self.read();
        if kolme.get_next_height() != signed_block.height() {
            anyhow::bail!(
                "Tried to add block with height {}, but next expected height is {}",
                signed_block.height(),
                kolme.get_next_height()
            );
        }
        let expected_processor = kolme.get_framework_state().get_validator_set().processor;
        let actual_processor = signed_block.0.message.as_inner().processor;
        anyhow::ensure!(expected_processor == actual_processor, "Received block signed by processor {actual_processor}, but the real processor is {expected_processor}");

        let txhash = signed_block.tx().hash();
        signed_block.validate_signature()?;
        let block = signed_block.0.message.as_inner();
        let ExecutionResults {
            framework_state,
            app_state,
            logs,
            loads,
        } = self
            .read()
            .execute_transaction(
                &block.tx,
                block.timestamp,
                BlockDataHandling::PriorData {
                    loads: signed_block.0.message.as_inner().loads.clone().into(),
                    validation: data_load_validation,
                },
            )
            .await?;

        anyhow::ensure!(loads == block.loads);

        let framework_state = Arc::new(framework_state);
        let app_state = Arc::new(app_state);
        let logs: Arc<[_]> = logs.into();

        self.inner
            .store
            .add_block(
                &self.inner.merkle_manager,
                StorableBlock {
                    height: signed_block.height().0,
                    blockhash: signed_block.hash().0,
                    txhash: signed_block.tx().hash().0,
                    block: signed_block.clone(),
                    framework_state: framework_state.clone(),
                    app_state: app_state.clone(),
                    logs: logs.clone(),
                },
            )
            .await?;

        self.inner.mempool.drop_tx(txhash);

        // Now do the write lock
        let mut guard = self.inner.current_block.write();

        if guard.get_next_height() > signed_block.height() {
            return Ok(());
        }

        *guard = Arc::new(MaybeBlockInfo::Some(BlockInfo {
            block: signed_block.clone(),
            logs,
            state: BlockState {
                blockhash: signed_block.hash(),
                framework_state,
                app_state,
            },
        }));
        std::mem::drop(guard);

        self.notify(Notification::NewBlock(signed_block));

        Ok(())
    }

    /// Validate and append the given block.
    pub async fn add_block_with_state(
        &self,
        signed_block: Arc<SignedBlock<App::Message>>,
        framework_state: Arc<MerkleContents>,
        app_state: Arc<MerkleContents>,
        logs: Arc<MerkleContents>,
    ) -> Result<()> {
        // Don't accept blocks we already have
        let kolme = self.read();
        if kolme.get_next_height() > signed_block.height() {
            anyhow::bail!(
                "Tried to add block (with state) with height {}, but next expected height is {}",
                signed_block.height(),
                kolme.get_next_height()
            );
        }
        let expected_processor = kolme.get_framework_state().get_validator_set().processor;
        let actual_processor = signed_block.0.message.as_inner().processor;
        anyhow::ensure!(expected_processor == actual_processor, "Received block signed by processor {actual_processor}, but the real processor is {expected_processor}");

        let txhash = signed_block.tx().hash();
        signed_block.validate_signature()?;
        let block = signed_block.0.message.as_inner();

        let framework_state = Arc::new(
            self.inner
                .store
                .store_and_load::<FrameworkState>(
                    &self.inner.merkle_manager,
                    block.framework_state,
                    framework_state,
                )
                .await?,
        );
        let app_state = Arc::new(
            self.inner
                .store
                .store_and_load::<App::State>(
                    &self.inner.merkle_manager,
                    block.app_state,
                    app_state,
                )
                .await?,
        );
        let logs = Arc::<[Vec<String>]>::from(
            self.inner
                .store
                .store_and_load::<Vec<Vec<String>>>(&self.inner.merkle_manager, block.logs, logs)
                .await?,
        );

        self.inner
            .store
            .add_block(
                &self.inner.merkle_manager,
                StorableBlock {
                    height: signed_block.height().0,
                    blockhash: signed_block.hash().0,
                    txhash: signed_block.tx().hash().0,
                    block: signed_block.clone(),
                    framework_state: framework_state.clone(),
                    app_state: app_state.clone(),
                    logs: logs.clone(),
                },
            )
            .await?;

        self.inner.mempool.drop_tx(txhash);

        // Now do the write lock
        let mut guard = self.inner.current_block.write();

        if guard.get_next_height() > signed_block.height() {
            return Ok(());
        }

        *guard = Arc::new(MaybeBlockInfo::Some(BlockInfo {
            block: signed_block.clone(),
            logs,
            state: BlockState {
                blockhash: signed_block.hash(),
                framework_state,
                app_state,
            },
        }));
        std::mem::drop(guard);

        self.notify(Notification::NewBlock(signed_block));

        Ok(())
    }

    pub async fn wait_on_mempool(&self) -> Arc<SignedTransaction<App::Message>> {
        loop {
            let (txhash, tx) = self.inner.mempool.peek().await;
            match self.get_tx_height(txhash).await {
                Ok(Some(_)) => self.inner.mempool.drop_tx(txhash),
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

    /// Wait for the mempool to be empty.
    ///
    /// This is mostly intended for writing tests.
    pub async fn wait_on_empty_mempool(&self) {
        self.inner.mempool.wait_for_pool_size(0).await;
    }

    /// Remove a transaction from the mempool, if present.
    ///
    /// If the transaction identified by `hash` is not present in the mempool,
    /// this function will silently do nothing. This behavior is intentional
    /// and ensures that calling this function is safe even if the transaction
    /// has already been removed or was never added.
    pub fn remove_from_mempool(&self, hash: TxHash) {
        self.inner.mempool.drop_tx(hash);
    }

    pub async fn new(
        app: App,
        _code_version: impl AsRef<str>,
        store: KolmeStore<App>,
    ) -> Result<Self> {
        // FIXME in the future do some validation of code version, and allow
        // for explicit events for upgrading to a newer code version
        let merkle_manager = MerkleManager::default();
        let current_block =
            MaybeBlockInfo::<App>::load(&store, app.genesis_info(), &merkle_manager).await?;
        let inner = KolmeInner {
            store,
            app,
            cosmos_conns: tokio::sync::RwLock::new(HashMap::new()),
            solana_conns: tokio::sync::RwLock::new(HashMap::new()),
            #[cfg(feature = "pass_through")]
            pass_through_conn: OnceLock::new(),
            merkle_manager,
            notify: tokio::sync::broadcast::channel(100).0,
            mempool: Mempool::new(),
            current_block: RwLock::new(Arc::new(current_block)),
            solana_endpoints: parking_lot::RwLock::new(SolanaEndpoints::default()),
        };

        let kolme = Kolme {
            inner: Arc::new(inner),
            tx_await_duration: tokio::time::Duration::from_secs(10),
        };

        kolme.resync().await?;
        kolme
            .inner
            .store
            .validate_genesis_info(&kolme.inner.merkle_manager, kolme.get_app().genesis_info())
            .await?;

        Ok(kolme)
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Notification<App::Message>> {
        self.inner.notify.subscribe()
    }

    /// Subscribe to get a notification each time an entry is added to the mempool.
    pub fn subscribe_mempool_additions(&self) -> tokio::sync::watch::Receiver<usize> {
        self.inner.mempool.subscribe_additions()
    }

    /// Get all entries currently in the mempool
    pub fn get_mempool_entries(&self) -> Vec<Arc<SignedTransaction<App::Message>>> {
        self.inner.mempool.get_entries()
    }

    /// Wait until the given block is published
    pub async fn wait_for_block(
        &self,
        height: BlockHeight,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        // First subscribe to avoid a race condition...
        let mut recv = self.inner.store.subscribe();
        loop {
            // And then check if we're at the requested height.
            if let Some(storable_block) = self.get_block(height).await? {
                return Ok(storable_block.block);
            }

            // Wait for more data
            recv.changed()
                .await
                .context("wait_for_block: unexpected end of stream from store.subscribe()")?;
        }
    }

    /// Wait until the given transaction is published
    pub async fn wait_for_tx(&self, tx: TxHash) -> Result<BlockHeight> {
        // Start an outer loop so that we can keep processing if we end up Lagged
        loop {
            // First subscribe to avoid a race condition...
            let mut recv = self.subscribe();
            // And then check if we have that transaction.
            if let Some(height) = self.read().get_tx_height(tx).await? {
                break Ok(height);
            }
            loop {
                match recv.recv().await {
                    Ok(note) => match note {
                        Notification::NewBlock(block) => {
                            if block.0.message.as_inner().tx.hash() == tx {
                                return Ok(block.0.message.as_inner().height);
                            }
                        }
                        Notification::GenesisInstantiation { .. } => (),
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

    /// Wait for the given public key to have an account ID and then return it.
    pub async fn wait_account_for_key(&self, pubkey: PublicKey) -> Result<AccountId> {
        loop {
            let kolme = self.read();
            if let Some((id, _)) = kolme
                .get_framework_state()
                .accounts
                .get_account_for_key(pubkey)
            {
                break Ok(id);
            }

            self.wait_for_block(kolme.get_next_height()).await?;
        }
    }

    /// Wait until the given action ID is no longer pending.
    pub async fn wait_for_action_finished(
        &self,
        chain: ExternalChain,
        action_id: BridgeActionId,
    ) -> Result<()> {
        loop {
            let kolme = self.read();
            let state = kolme.get_framework_state().chains.get(chain)?;

            // Check that we've already issued the action, _and_ that the action
            // isn't pending.
            if state.next_action_id > action_id && !state.pending_actions.contains_key(&action_id) {
                break Ok(());
            }

            // Either we haven't actually issued that action yet, or the action is still pending.
            // Either way, wait for another block and then try again.

            self.wait_for_block(kolme.get_next_height()).await?;
        }
    }

    pub async fn get_log_events_for(&self, height: BlockHeight) -> Result<Vec<LogEvent>> {
        let block = self
            .get_block(height)
            .await?
            .with_context(|| format!("get_log_events_for({height}: block not available"))?;
        Ok(block
            .logs
            .iter()
            .flatten()
            .flat_map(|s| serde_json::from_str::<LogEvent>(s).ok())
            .collect())
    }

    pub async fn get_cosmos(&self, chain: CosmosChain) -> Result<cosmos::Cosmos> {
        if let Some(cosmos) = self.inner.cosmos_conns.read().await.get(&chain) {
            return Ok(cosmos.clone());
        }

        let mut guard = self.inner.cosmos_conns.write().await;
        match guard.get(&chain) {
            Some(cosmos) => Ok(cosmos.clone()),
            None => {
                let cosmos = chain.make_client().await?;
                guard.insert(chain, cosmos.clone());
                Ok(cosmos)
            }
        }
    }

    /// Sets a Solana endpoint for regular (non-pubsub) connections.
    ///
    /// # Parameters
    /// - `chain`: The Solana chain for which the endpoint is being set.
    /// - `endpoint`: The URL of the Solana endpoint to use for regular connections.
    ///
    /// # Usage
    /// Call this method to specify a custom Solana endpoint for regular connections.
    /// If no custom endpoint is set, a default endpoint will be used.
    pub fn set_solana_endpoint_regular(&self, chain: SolanaChain, endpoint: impl Into<Arc<str>>) {
        self.inner
            .solana_endpoints
            .write()
            .regular
            .insert(chain, endpoint.into());
    }

    /// Set a Solana endpoint for pubsub connections.
    pub fn set_solana_endpoint_pubsub(&self, chain: SolanaChain, endpoint: impl Into<Arc<str>>) {
        self.inner
            .solana_endpoints
            .write()
            .pubsub
            .insert(chain, endpoint.into());
    }

    pub async fn get_solana_client(&self, chain: SolanaChain) -> Arc<SolanaClient> {
        if let Some(client) = self.inner.solana_conns.read().await.get(&chain) {
            return client.clone();
        }

        let mut guard = self.inner.solana_conns.write().await;
        match guard.get(&chain) {
            Some(client) => Arc::clone(client),
            None => {
                let client = Arc::new(
                    self.inner
                        .solana_endpoints
                        .read()
                        .get_regular_endpoint(chain)
                        .make_client(),
                );
                guard.insert(chain, Arc::clone(&client));

                client
            }
        }
    }

    pub async fn get_solana_pubsub_client(&self, chain: SolanaChain) -> Result<PubsubClient> {
        // TODO do we need caching here?

        let endpoint = self
            .inner
            .solana_endpoints
            .read()
            .get_pubsub_endpoint(chain);
        endpoint.make_pubsub_client().await
    }

    #[cfg(feature = "pass_through")]
    pub async fn get_pass_through_client(&self) -> reqwest::Client {
        self.inner
            .pass_through_conn
            .get_or_init(reqwest::Client::new)
            .clone()
    }

    pub fn get_app(&self) -> &App {
        &self.inner.app
    }

    /// Take a lock on constructing new blocks.
    pub(crate) async fn take_construct_lock(&self) -> Result<KolmeConstructLock> {
        self.inner.store.take_construct_lock().await
    }

    /// Returns a hash of the genesis info.
    ///
    /// Purpose: this provides a unique identifier for a chain.
    pub fn get_genesis_hash(&self) -> Result<Sha256Hash> {
        let info = self.inner.app.genesis_info();
        let info = serde_json::to_vec(info)?;
        Ok(Sha256Hash::hash(&info))
    }
}

impl<App: KolmeApp> Kolme<App> {
    /// Returns the given block, if available.
    pub async fn get_block(
        &self,
        height: BlockHeight,
    ) -> Result<Option<StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>> {
        self.inner
            .store
            .load_block(&self.inner.merkle_manager, height)
            .await
    }

    /// Get the block height for the given transaction, if present.
    pub async fn get_tx_height(&self, tx: TxHash) -> Result<Option<BlockHeight>> {
        self.inner.store.get_height_for_tx(tx).await
    }

    /// Get the [MerkleManager]
    pub fn get_merkle_manager(&self) -> &MerkleManager {
        &self.inner.merkle_manager
    }

    /// Load the block details from the database
    pub async fn load_block(
        &self,
        height: BlockHeight,
    ) -> Result<StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>> {
        self.get_block(height)
            .await?
            .ok_or(KolmeStoreError::BlockNotFound { height: height.0 }.into())
    }
}

impl<App: KolmeApp> KolmeRead<App> {
    pub fn get_next_height(&self) -> BlockHeight {
        self.current.get_next_height()
    }

    /// Returns the hash of the most recent block.
    ///
    /// If there is no event present, returns the special parent for the genesis block.
    pub fn get_current_block_hash(&self) -> BlockHash {
        self.current.get_block_hash()
    }

    pub fn get_next_genesis_action(&self) -> Option<GenesisAction> {
        for (chain, state) in self.get_framework_state().chains.iter() {
            match &state.config.bridge {
                BridgeContract::NeededCosmosBridge { code_id } => {
                    return Some(GenesisAction::InstantiateCosmos {
                        chain: chain.to_cosmos_chain().unwrap(),
                        code_id: *code_id,
                        validator_set: self.get_framework_state().get_validator_set().clone(),
                    })
                }
                BridgeContract::NeededSolanaBridge { program_id } => {
                    return Some(GenesisAction::InstantiateSolana {
                        chain: chain.to_solana_chain().unwrap(),
                        program_id: program_id.clone(),
                        validator_set: self.get_framework_state().get_validator_set().clone(),
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
            .get_framework_state()
            .chains
            .get(chain)?
            .pending_actions
            .iter()
            .next()
            .map(|(k, v)| (*k, v)))
    }

    pub fn get_app_state(&self) -> &App::State {
        self.current.get_app_state()
    }

    pub fn get_framework_state(&self) -> &FrameworkState {
        self.current.get_framework_state()
    }

    pub fn get_processor_pubkey(&self) -> PublicKey {
        self.get_framework_state().get_validator_set().processor
    }

    pub fn get_approver_pubkeys(&self) -> &BTreeSet<PublicKey> {
        &self.get_framework_state().get_validator_set().approvers
    }

    pub fn get_needed_approvers(&self) -> u16 {
        self.get_framework_state()
            .get_validator_set()
            .needed_approvers
    }

    pub fn get_bridge_contracts(&self) -> &ChainStates {
        &self.get_framework_state().chains
    }

    pub fn get_balances(&self) -> &Accounts {
        &self.get_framework_state().accounts
    }

    pub fn get_account_balances(
        &self,
        account_id: &AccountId,
    ) -> Option<&BTreeMap<AssetId, Decimal>> {
        self.get_framework_state().accounts.get_assets(account_id)
    }

    pub fn create_signed_transaction(
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

    /// Get the next nonce to be used for the account associated with this public key.
    ///
    /// This function is read-only, and works for both accounts that do and don't exist.
    ///
    /// For new accounts, it will always return the initial nonce.
    pub fn get_next_nonce(&self, key: PublicKey) -> AccountNonce {
        self.get_framework_state()
            .accounts
            .get_account_for_key(key)
            .map_or_else(AccountNonce::start, |(_, account)| account.get_next_nonce())
    }

    pub fn get_admin_proposal_payload(
        &self,
        proposal_id: AdminProposalId,
    ) -> Option<&ProposalPayload> {
        self.get_framework_state()
            .admin_proposal_state
            .as_ref()
            .proposals
            .get(&proposal_id)
            .map(|p| &p.payload)
    }
}
