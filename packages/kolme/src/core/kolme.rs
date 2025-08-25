mod block_info;
mod import_export;
mod mempool;
mod store;

use block_info::BlockState;
pub(super) use block_info::{BlockInfo, MaybeBlockInfo};
use kolme_store::{KolmeConstructLock, KolmeStoreError, StorableBlock};
use parking_lot::RwLock;
#[cfg(feature = "solana")]
use solana_client::nonblocking::pubsub_client::PubsubClient;
pub use store::KolmeStore;
use utils::trigger::TriggerSubscriber;

#[cfg(any(feature = "solana", feature = "cosmwasm"))]
use std::collections::HashMap;
use std::{num::NonZero, ops::Deref, sync::OnceLock, time::Duration};

use mempool::Mempool;
pub use mempool::ProposeTransactionError;

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
    mempool: Mempool<App::Message>,
    pub(super) store: store::KolmeStore<App>,
    pub(super) app: App,
    #[cfg(feature = "cosmwasm")]
    pub(super) cosmos_conns: tokio::sync::RwLock<HashMap<CosmosChain, cosmos::Cosmos>>,
    #[cfg(feature = "solana")]
    pub(super) solana_conns: tokio::sync::RwLock<HashMap<SolanaChain, Arc<SolanaClient>>>,
    #[cfg(feature = "solana")]
    pub(super) solana_endpoints: parking_lot::RwLock<SolanaEndpoints>,
    #[cfg(feature = "pass_through")]
    pub(super) pass_through_conn: OnceLock<reqwest::Client>,
    current_block: RwLock<Arc<MaybeBlockInfo<App>>>,
    /// Latest block reported by the processor
    pub(super) latest_block: tokio::sync::watch::Sender<Option<Arc<SignedTaggedJson<LatestBlock>>>>,
    /// The version of the chain the current codebase represents.
    ///
    /// If this version is different from the active version running on the chain,
    /// we cannot produce blocks or execute blocks with reproducibility.
    code_version: String,
    /// A channel for requesting new blocks to be synced from the network.
    block_requester: OnceLock<tokio::sync::mpsc::Sender<BlockHeight>>,
    failed_txs: tokio::sync::broadcast::Sender<Arc<SignedTaggedJson<FailedTransaction>>>,
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

    /// Subscribe to wait for a new block to become available.
    pub fn subscribe_new_block(&self) -> TriggerSubscriber {
        // TODO confirm if this is correct, it may include any added Merkle layers too, which may be too many notifications
        self.inner.store.subscribe()
        // TODO need to also add some kind of listener for PostgreSQL to notify us when another node adds a block to a shared database
    }

    /// Subscribe to failed transaction notifications from the processor.
    pub fn subscribe_failed_txs(
        &self,
    ) -> tokio::sync::broadcast::Receiver<Arc<SignedTaggedJson<FailedTransaction>>> {
        self.inner.failed_txs.subscribe()
    }

    /// Subscribe for notifications of new latest block information.
    pub fn subscribe_latest_block(
        &self,
    ) -> tokio::sync::watch::Receiver<Option<Arc<SignedTaggedJson<LatestBlock>>>> {
        self.inner.latest_block.subscribe()
    }

    pub(crate) fn update_latest_block(&self, latest_block: Arc<SignedTaggedJson<LatestBlock>>) {
        if let Err(e) = self.verify_processor_signature(&latest_block) {
            tracing::warn!("Invalid processor signature on signed latest block: {e}");
        }

        // Returns Ok if we can proceed with overwriting the old latest, Err otherwise
        fn check_height_when(
            old_latest: &SignedTaggedJson<LatestBlock>,
            latest_block: &SignedTaggedJson<LatestBlock>,
        ) -> Result<(), ()> {
            let old_latest = old_latest.message.as_inner();
            let old_height = old_latest.height;
            let old_when = old_latest.when;

            let new_latest = latest_block.message.as_inner();
            let new_height = new_latest.height;
            let new_when = new_latest.when;

            if new_height < old_height {
                tracing::warn!(
                    "Got a latest block of {new_height}, which is less than last known value of {old_height}"
                );
                Err(())
            } else if old_when >= new_when {
                Err(())
            } else {
                Ok(())
            }
        }

        self.inner.latest_block.send_if_modified(|old_latest| {
            if let Some(old_latest) = old_latest.as_ref() {
                if check_height_when(old_latest, &latest_block).is_err() {
                    return false;
                }
            }

            *old_latest = Some(latest_block);
            true
        });
    }

    /// Propose a new transaction for the processor to add to the chain.
    ///
    /// Note that this will not detect any issues if the transaction is rejected.
    pub fn propose_transaction(
        &self,
        tx: Arc<SignedTransaction<App::Message>>,
    ) -> Result<(), ProposeTransactionError<App::Message>> {
        self.inner.mempool.add(tx)
    }

    fn verify_processor_signature<T>(&self, signed: &SignedTaggedJson<T>) -> Result<()> {
        // Note that during a key rotation, we will have a switch in the
        // processor, and during that period some signatures will be
        // incorrectly excluded. That's acceptable, we expect the new block data
        // to come in quickly.

        // Validate the signature
        let pubkey = signed.verify_signature()?;

        let processor = self
            .read()
            .get_framework_state()
            .validator_set
            .as_ref()
            .processor;
        if pubkey == processor {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Latest block was signed by {pubkey}, but processor is {processor}"
            ))
        }
    }

    /// Log a failed transaction.
    pub fn add_failed_transaction(&self, failed: Arc<SignedTaggedJson<FailedTransaction>>) {
        if let Err(e) = self.verify_processor_signature(&failed) {
            tracing::warn!("Invalid processor signature on signed failed transaction: {e}");
        }
        if self.inner.mempool.add_failed_transaction(failed.clone()) {
            self.inner.failed_txs.send(failed).ok();
        }
    }

    /// Remove an entry from the mempool.
    pub fn remove_mempool_entry(&self, txhash: TxHash) {
        self.inner.mempool.remove(txhash);
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
        let mut new_block = self.subscribe_new_block();
        let mut failed_tx = self.subscribe_failed_txs();
        let txhash = tx.hash();
        loop {
            match self.propose_transaction(tx.clone()) {
                // The only way this should happen is if the transaction was
                // booted from the LRU cache. In that case, just try again.
                Ok(()) => (),
                // Still in the mempool, so continue waiting
                Err(ProposeTransactionError::InMempool) => (),
                Err(ProposeTransactionError::InBlock(block)) => {
                    debug_assert_eq!(block.tx().hash(), txhash);
                    break Ok(block);
                }
                Err(ProposeTransactionError::Failed(failed)) => {
                    debug_assert_eq!(failed.message.as_inner().txhash, txhash);
                    break Err(failed.message.as_inner().error.clone().into());
                }
            }

            // Wait until we either get a new block or a new failed notification comes in.
            tokio::select! {
                _ = new_block.listen() => (),
                _ = failed_tx.recv() => (),
            };

            // There's a potential race condition, the transaction could have be flushed
            // from the LRU cache after successfully landing in a block. No worries if that
            // occurs, we'll try proposing again and will eventually be told by another node
            // that it landed in a block.
        }
    }

    /// Sign and propose a transaction.
    ///
    /// Automatically resigns with a new nonce if necessary.
    pub async fn sign_propose_await_transaction<T: Into<TxBuilder<App::Message>>>(
        &self,
        secret: &SecretKey,
        tx_builder: T,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        match tokio::time::timeout(
            self.tx_await_duration,
            self.sign_propose_await_transaction_inner(secret, tx_builder.into()),
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
        tx_builder: TxBuilder<App::Message>,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        let pubkey = secret.public_key();
        let (next_block_height, mut nonce) = {
            let kolme_r = self.read();
            let next_block_height = kolme_r.get_next_height();
            let nonce = kolme_r.get_next_nonce(pubkey).1;
            (next_block_height, nonce)
        };
        let mut attempt = 1;
        const MAX_NONCE_ATTEMPTS: usize = 5;
        loop {
            let tx = Arc::new(self.read().create_signed_transaction_with(
                secret,
                tx_builder.clone(),
                pubkey,
                nonce,
            )?);
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
                        if actual < expected && attempt < MAX_NONCE_ATTEMPTS {
                            tracing::warn!("Retrying with new nonce, attempt {attempt}/{MAX_NONCE_ATTEMPTS}. Retrieved attempted nonce from framework state with next_block_height {next_block_height}. Error: {e}");
                            attempt += 1;
                            nonce = *expected;
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
        if let Some(height) = self.inner.store.load_latest_block().await? {
            if self.read().get_next_height() < height.next() {
                let block = self
                    .inner
                    .store
                    .load_signed_block(height)
                    .await?
                    .with_context(|| format!("Expected block {height} not found during resync"))?;

                let (framework_state, app_state) = tokio::try_join!(
                    self.load_framework_state(block.as_inner().framework_state),
                    self.load_app_state(block.as_inner().app_state)
                )?;

                let state = BlockState {
                    blockhash: block.hash(),
                    framework_state: Arc::new(framework_state),
                    app_state: Arc::new(app_state),
                };
                let block_info = BlockInfo { block, state };

                let mut guard = self.inner.current_block.write();
                if guard.get_next_height() < height.next() {
                    *guard = Arc::new(MaybeBlockInfo::Some(block_info));
                }
            }
        }
        Ok(())
    }

    /// Validate and append the given block.
    ///
    /// Responsible for validating signatures and state transitions.
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
        // FIXME add support for adding old blocks instead
        if kolme.get_next_height() != signed_block.height() {
            anyhow::bail!(
                "Tried to add block with height {}, but next expected height is {}",
                signed_block.height(),
                kolme.get_next_height()
            );
        }

        let actual_parent = kolme.get_current_block_hash();
        let block_parent = signed_block.0.message.as_inner().parent;
        anyhow::ensure!(
            actual_parent == block_parent,
            "Tried to add block height {}, but actual parent has block hash {actual_parent} and block specifies {block_parent}",
            signed_block.height()
        );

        let expected_processor = kolme.get_framework_state().get_validator_set().processor;
        let actual_processor = signed_block.0.message.as_inner().processor;
        anyhow::ensure!(
            expected_processor == actual_processor,
            "Received block signed by processor {actual_processor}, but the real processor is {expected_processor}"
        );

        // Ensure the max height is respected if present
        if let Some(max_height) = signed_block.tx().0.message.as_inner().max_height {
            if max_height < signed_block.height() {
                return Err(KolmeError::PastMaxHeight {
                    txhash: signed_block.tx().hash(),
                    max_height,
                    proposed_height: signed_block.height(),
                }
                .into());
            }
        }

        signed_block.validate_signature()?;
        let block = signed_block.0.message.as_inner();
        let ExecutionResults {
            framework_state,
            app_state,
            logs,
            loads,
            height,
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
        anyhow::ensure!(height == signed_block.height());
        anyhow::ensure!(loads == block.loads);

        self.add_executed_block(ExecutedBlock {
            signed_block,
            framework_state,
            app_state,
            logs,
        })
        .await
    }

    /// Add a block that has already been executed.
    ///
    /// This allows the processor to skip immediate revalidation when execution has already been performed.
    pub(crate) async fn add_executed_block(
        &self,
        executed_block: ExecutedBlock<App>,
    ) -> Result<()> {
        let ExecutedBlock {
            signed_block,
            framework_state,
            app_state,
            logs,
        } = executed_block;
        let framework_state = Arc::new(framework_state);
        let app_state = Arc::new(app_state);
        let logs: Arc<[_]> = logs.into();
        let height = signed_block.height();

        let framework_state_hash = self.inner.store.save(&framework_state).await?;
        anyhow::ensure!(framework_state_hash == signed_block.0.message.as_inner().framework_state);

        let app_state_hash = self.inner.store.save(&app_state).await?;
        anyhow::ensure!(app_state_hash == signed_block.0.message.as_inner().app_state);

        let logs_hash = self.inner.store.save(&logs).await?;
        anyhow::ensure!(logs_hash == signed_block.0.message.as_inner().logs);

        self.inner
            .store
            .add_block(StorableBlock {
                height: signed_block.height().0,
                blockhash: signed_block.hash().0,
                txhash: signed_block.tx().hash().0,
                block: signed_block.clone(),
            })
            .await?;

        self.inner.mempool.add_signed_block(signed_block.clone());

        // Now do the write lock
        {
            let mut guard = self.inner.current_block.write();

            if guard.get_next_height() > signed_block.height() {
                return Ok(());
            }

            *guard = Arc::new(MaybeBlockInfo::Some(BlockInfo {
                block: signed_block.clone(),
                state: BlockState {
                    blockhash: signed_block.hash(),
                    framework_state,
                    app_state,
                },
            }));
        }

        // Update the archive if appropriate
        if self.get_next_to_archive().await? == height {
            if let Err(e) = self.inner.store.archive_block(height).await {
                tracing::warn!("Unable to mark block {height} as archived: {e}");
            }
        }

        Ok(())
    }

    /// Validate and append the given block.
    ///
    /// Note that this does not execute the transaction, since this
    /// is used by state sync and other cases where such execution is
    /// not necessarily possible. Instead, it validates signatures
    /// and hashes.
    ///
    /// The state values must already be in the store.
    ///
    /// Note that for efficiency reasons, this method will not automatically
    /// load up the block for the next [Kolme::read] call. If you need to work
    /// with that block, you can use [Kolme::resync] to force the latest block to load.
    pub async fn add_block_with_state(
        &self,
        signed_block: Arc<SignedBlock<App::Message>>,
    ) -> Result<()> {
        // Don't accept blocks we already have
        if self.has_block(signed_block.height()).await? {
            anyhow::bail!(
                "Tried to add block with height {}, but it's already present in the store.",
                signed_block.height()
            );
        }
        let kolme = self.read();
        let expected_processor = kolme.get_framework_state().get_validator_set().processor;
        let actual_processor = signed_block.0.message.as_inner().processor;
        anyhow::ensure!(
            expected_processor == actual_processor,
            "Received block signed by processor {actual_processor}, but the real processor is {expected_processor}"
        );

        signed_block.validate_signature()?;
        let block = signed_block.0.message.as_inner();

        anyhow::ensure!(
            self.has_merkle_hash(block.framework_state).await?,
            "Framework state {} not written to Merkle store",
            block.framework_state
        );
        anyhow::ensure!(
            self.has_merkle_hash(block.app_state).await?,
            "App state {} not written to Merkle store",
            block.app_state
        );
        anyhow::ensure!(
            self.has_merkle_hash(block.logs).await?,
            "Logs {} not written to Merkle store",
            block.logs
        );

        self.inner
            .store
            .add_block(StorableBlock {
                height: signed_block.height().0,
                blockhash: signed_block.hash().0,
                txhash: signed_block.tx().hash().0,
                block: signed_block.clone(),
            })
            .await?;

        self.inner.mempool.add_signed_block(signed_block.clone());

        Ok(())
    }

    pub async fn wait_on_mempool(&self) -> Arc<SignedTransaction<App::Message>> {
        loop {
            let tx = self.inner.mempool.peek().await;
            let txhash = tx.hash();
            match self.get_tx_block(txhash).await {
                Ok(Some(block)) => {
                    self.inner.mempool.add_signed_block(block);
                }
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

    pub async fn new(
        app: App,
        code_version: impl Into<String>,
        store: KolmeStore<App>,
    ) -> Result<Self> {
        let current_block = MaybeBlockInfo::<App>::load(&store, &app).await?;
        let inner = KolmeInner {
            store,
            app,
            #[cfg(feature = "cosmwasm")]
            cosmos_conns: tokio::sync::RwLock::new(HashMap::new()),
            #[cfg(feature = "solana")]
            solana_conns: tokio::sync::RwLock::new(HashMap::new()),
            #[cfg(feature = "pass_through")]
            pass_through_conn: OnceLock::new(),
            // In the future, maybe have a Builder interface for configuring things like this
            // Default value chosen to exceed the libp2p default of 60 seconds
            mempool: Mempool::new(
                Duration::from_secs(90),
                NonZero::new(1024).unwrap(),
                NonZero::new(1024).unwrap(),
            ),
            current_block: RwLock::new(Arc::new(current_block)),
            #[cfg(feature = "solana")]
            solana_endpoints: parking_lot::RwLock::new(SolanaEndpoints::default()),
            latest_block: tokio::sync::watch::Sender::new(None),
            code_version: code_version.into(),
            block_requester: OnceLock::new(),
            failed_txs: tokio::sync::broadcast::Sender::new(100),
        };

        let kolme = Kolme {
            inner: Arc::new(inner),
            tx_await_duration: tokio::time::Duration::from_secs(10),
        };

        kolme
            .inner
            .store
            .validate_genesis_info(kolme.get_app().genesis_info())
            .await?;

        Ok(kolme)
    }

    /// Subscribe to get a notification each time an entry is added to the mempool.
    pub fn subscribe_mempool_additions(&self) -> TriggerSubscriber {
        self.inner.mempool.subscribe_additions()
    }

    /// Get all entries currently in the mempool
    pub fn get_mempool_entries(&self) -> Vec<Arc<SignedTransaction<App::Message>>> {
        self.inner.mempool.get_entries()
    }

    /// Get all mempool entries which should be gossiped.
    ///
    /// This excludes entries which have been seen gossiped recently.
    pub fn get_mempool_entries_for_gossip(&self) -> Vec<Arc<SignedTransaction<App::Message>>> {
        self.inner.mempool.get_entries_for_gossip()
    }

    /// Mark a mempool transaction as having been gossiped.
    ///
    /// This prevents the transaction from being rebroadcast too frequently.
    pub fn mark_mempool_entry_gossiped(&self, txhash: TxHash) {
        self.inner.mempool.mark_mempool_entry_gossiped(txhash)
    }

    /// Wait until the given block is published
    pub async fn wait_for_block(
        &self,
        height: BlockHeight,
    ) -> Result<Arc<SignedBlock<App::Message>>> {
        // Optimization for the common case.
        if let Some(storable_block) = self.get_block(height).await? {
            return Ok(storable_block.block);
        }

        // First subscribe to avoid a race condition...
        let mut recv = self.inner.store.subscribe();
        let mut last_warning = std::time::Instant::now();
        loop {
            // And then check if we're at the requested height.
            if let Some(storable_block) = self.get_block(height).await? {
                return Ok(storable_block.block);
            }

            if let Some(requester) = self.inner.block_requester.get() {
                requester.send(height).await.ok();
            }

            // Only wait up to 5 seconds for a new event, if that doesn't happen,
            // check again in case an archiver or similar filled in an old
            // block without triggering an event.
            //
            // TODO: investigate this more closely, maybe we need gossip to generate
            // a notification every time a new block is added.
            tokio::time::timeout(tokio::time::Duration::from_secs(5), recv.listen())
                .await
                .ok();

            // If we've waited too long, print a warning.
            let now = std::time::Instant::now();
            if now.duration_since(last_warning).as_secs() >= 30 {
                tracing::warn!("Still waiting for block {height}");
                last_warning = now;
            };
        }
    }

    /// Set the block requester
    ///
    /// Current kept pub(crate) as it's only used by the gossip mechanism.
    ///
    /// If there's already a block requester set, this is a no-op.
    pub(crate) fn set_block_requester(&self, requester: tokio::sync::mpsc::Sender<BlockHeight>) {
        self.inner.block_requester.set(requester).ok();
    }

    /// Wait until the given transaction is published
    pub async fn wait_for_tx(&self, tx: TxHash) -> Result<BlockHeight> {
        let mut new_block = self.subscribe_new_block();
        loop {
            if let Some(height) = self.get_tx_height(tx).await? {
                return Ok(height);
            }
            new_block.listen().await;
        }
    }

    /// Wait for the chain to be running on the version we are expecting to see.
    pub async fn wait_for_active_version(&self) {
        // FIXME didn't we want some kind of advertisement of new versions or something like that?

        // First subscribe to avoid a race condition...
        let mut new_block = self.subscribe_new_block();
        loop {
            // TODO: Consider if we need a better mechanism overall here.
            // We resync here because it's possible that we added a new block via state sync but haven't loaded it yet. The fact that this bug could exist indicates we should have a better notification and block loading system.
            if let Err(e) = self.resync().await {
                tracing::error!("Error resyncing in wait_for_active_version: {e}");
            }

            // And then check if we are at the desired version.
            if self.read().get_chain_version() == self.get_code_version() {
                return;
            }
            new_block.listen().await;
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

    /// Wait for the given wallet to have an account ID and then return it.
    pub async fn wait_account_for_wallet(&self, wallet: &Wallet) -> Result<AccountId> {
        loop {
            let kolme = self.read();
            if let Some((id, _)) = kolme
                .get_framework_state()
                .accounts
                .get_account_for_wallet(wallet)
            {
                break Ok(id);
            }

            self.wait_for_block(kolme.get_next_height()).await?;
        }
    }

    /// Wait until the given bridge event ID lands.
    pub async fn wait_for_bridge_event(
        &self,
        chain: ExternalChain,
        event_id: BridgeEventId,
    ) -> Result<()> {
        loop {
            let kolme = self.read();
            let state = kolme.get_framework_state().chains.get(chain)?;

            if state.next_event_id > event_id {
                break Ok(());
            }

            // Either we haven't actually issued that action yet, or the action is still pending.
            // Either way, wait for another block and then try again.

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
        let logs = self.load_logs(block.block.as_inner().logs).await?;
        Ok(logs
            .iter()
            .flatten()
            .flat_map(|s| serde_json::from_str::<LogEvent>(s).ok())
            .collect())
    }

    /// Load up logs by the given Merkle hash.
    pub async fn load_logs(&self, hash: Sha256Hash) -> Result<Vec<Vec<String>>, MerkleSerialError> {
        self.get_merkle_by_hash(hash).await
    }

    /// Load up framework state by the given Merkle hash.
    pub async fn load_framework_state(
        &self,
        hash: Sha256Hash,
    ) -> Result<FrameworkState, MerkleSerialError> {
        self.get_merkle_by_hash(hash).await
    }

    /// Load up app state by the given Merkle hash.
    pub async fn load_app_state(&self, hash: Sha256Hash) -> Result<App::State, MerkleSerialError> {
        self.get_merkle_by_hash(hash).await
    }

    #[cfg(feature = "cosmwasm")]
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
    #[cfg(feature = "solana")]
    pub fn set_solana_endpoint_regular(&self, chain: SolanaChain, endpoint: impl Into<Arc<str>>) {
        self.inner
            .solana_endpoints
            .write()
            .regular
            .insert(chain, endpoint.into());
    }

    /// Set a Solana endpoint for pubsub connections.
    #[cfg(feature = "solana")]
    pub fn set_solana_endpoint_pubsub(&self, chain: SolanaChain, endpoint: impl Into<Arc<str>>) {
        self.inner
            .solana_endpoints
            .write()
            .pubsub
            .insert(chain, endpoint.into());
    }

    #[cfg(feature = "solana")]
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

    #[cfg(feature = "solana")]
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
    pub fn get_pass_through_client(&self) -> reqwest::Client {
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

    /// Returns the genesis info
    pub fn get_genesis_info(&self) -> &GenesisInfo {
        self.inner.app.genesis_info()
    }

    /// Returns a hash of the genesis info.
    ///
    /// Purpose: this provides a unique identifier for a chain.
    pub fn get_genesis_hash(&self) -> Result<Sha256Hash> {
        let info = self.inner.app.genesis_info();
        let info = serde_json::to_vec(info)?;
        Ok(Sha256Hash::hash(&info))
    }

    /// Returns the latest block information from the processor.
    ///
    /// This may be different from the latest block known on this node if we
    /// haven't completed syncing yet. The processor generates these messages
    /// regularly to keep the rest of the chain aware of what the latest known
    /// height is at various timestamps to avoid drift.
    pub fn get_latest_block(&self) -> Option<Arc<SignedTaggedJson<LatestBlock>>> {
        self.inner.latest_block.borrow().clone()
    }

    pub fn get_code_version(&self) -> &String {
        &self.inner.code_version
    }

    /// Get the Merkle layer for this hash, if available.
    pub async fn get_merkle_layer(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleLayerContents>, MerkleSerialError> {
        // TODO make this return an Arc?
        self.inner.store.get_merkle_layer(hash).await
    }

    /// Add a Merkle layer for this hash.
    ///
    /// Invariant: you must ensure that all children are already stored.
    pub(crate) async fn add_merkle_layer(&self, layer: &MerkleLayerContents) -> Result<()> {
        self.inner.store.add_merkle_layer(layer).await
    }

    /// Get the contents of a Merkle hash.
    pub(crate) async fn get_merkle_by_hash<T: MerkleDeserializeRaw>(
        &self,
        hash: Sha256Hash,
    ) -> Result<T, MerkleSerialError> {
        self.inner.store.load(hash).await
    }

    /// Ingest all blocks from the given Kolme into this one.
    pub async fn ingest_blocks_from(&self, other: &Self) -> Result<()> {
        loop {
            let to_archive = self.get_next_to_archive().await?;
            let Some(block) = other.get_block(to_archive).await? else {
                self.resync().await?;
                break Ok(());
            };
            self.ingest_layer_from(other, block.block.as_inner().framework_state)
                .await?;
            self.ingest_layer_from(other, block.block.as_inner().app_state)
                .await?;
            self.ingest_layer_from(other, block.block.as_inner().logs)
                .await?;
            self.add_block_with_state(block.block).await?;
        }
    }

    async fn ingest_layer_from(&self, other: &Self, hash: Sha256Hash) -> Result<()> {
        enum Work {
            Process(Sha256Hash),
            Write(Box<MerkleLayerContents>),
        }
        let mut work_queue = vec![Work::Process(hash)];
        while let Some(work) = work_queue.pop() {
            match work {
                Work::Process(hash) => {
                    if self.has_merkle_hash(hash).await? {
                        continue;
                    }
                    let layer = other
                        .get_merkle_layer(hash)
                        .await?
                        .with_context(|| format!("Missing layer {hash} in source store"))?;
                    let children = layer.children.clone();
                    work_queue.push(Work::Write(Box::new(layer)));
                    for child in children {
                        work_queue.push(Work::Process(child));
                    }
                }
                Work::Write(layer) => {
                    if self.has_merkle_hash(layer.payload.hash()).await? {
                        continue;
                    }
                    self.add_merkle_layer(&layer).await?;
                }
            }
        }
        Ok(())
    }
}

impl<App: KolmeApp> Kolme<App> {
    /// Returns the given block, if available.
    pub async fn get_block(
        &self,
        height: BlockHeight,
    ) -> Result<Option<StorableBlock<SignedBlock<App::Message>>>> {
        self.inner.store.load_block(height).await
    }

    /// Check if the given block is available in storage.
    pub async fn has_block(&self, height: BlockHeight) -> Result<bool, KolmeStoreError> {
        self.inner.store.has_block(height).await
    }

    /// Check if the given Merkle hash is stored in the backing store.
    pub async fn has_merkle_hash(&self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        self.inner.store.has_merkle_hash(hash).await
    }

    /// Get the block height for the given transaction, if present.
    pub async fn get_tx_height(&self, tx: TxHash) -> Result<Option<BlockHeight>> {
        self.inner.store.get_height_for_tx(tx).await
    }

    /// Get the block containing the given transaction, if present.
    pub async fn get_tx_block(&self, tx: TxHash) -> Result<Option<Arc<SignedBlock<App::Message>>>> {
        let Some(height) = self.get_tx_height(tx).await? else {
            return Ok(None);
        };
        Ok(Some(self.load_block(height).await?.block))
    }

    /// Load the block details from the database
    pub async fn load_block(
        &self,
        height: BlockHeight,
    ) -> Result<StorableBlock<SignedBlock<App::Message>>> {
        self.get_block(height)
            .await?
            .ok_or(KolmeStoreError::BlockNotFound { height: height.0 }.into())
    }

    /// Marks the current block to not be resynced by the Archiver
    pub async fn archive_block(&self, height: BlockHeight) -> Result<()> {
        self.inner
            .store
            .archive_block(height)
            .await
            .with_context(|| format!("Unable to mark block {} as archived", height.0))
    }

    /// Obtains the latest block synced by the Archiver, if it exists
    pub async fn get_latest_archived_block(&self) -> Result<Option<BlockHeight>> {
        Ok(self
            .inner
            .store
            .get_latest_archived_block_height()
            .await
            .context("Unable to retrieve latest archived block height")?
            .map(BlockHeight))
    }

    /// Get the next block to archive.
    ///
    /// This will report errors during data load and then return the earliest
    /// block height, essentially restarting the archive process.
    pub async fn get_next_to_archive(&self) -> Result<BlockHeight> {
        let mut next = self
            .get_latest_archived_block()
            .await?
            .map_or_else(BlockHeight::start, BlockHeight::next);

        while self.has_block(next).await? {
            // Mark the "next" block as already archived.
            self.inner.store.archive_block(next).await?;
            tracing::info!("get_next_to_archive: block already in database: {next}");
            next = next.next();
        }
        Ok(next)
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
                    });
                }
                BridgeContract::NeededSolanaBridge { program_id } => {
                    return Some(GenesisAction::InstantiateSolana {
                        chain: chain.to_solana_chain().unwrap(),
                        program_id: program_id.clone(),
                        validator_set: self.get_framework_state().get_validator_set().clone(),
                    });
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

    pub fn get_code_version(&self) -> &String {
        self.kolme.get_code_version()
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

    pub fn create_signed_transaction<T: Into<TxBuilder<App::Message>>>(
        &self,
        secret: &SecretKey,
        tx_builder: T,
    ) -> Result<SignedTransaction<App::Message>> {
        let pubkey = secret.public_key();
        let nonce = self.get_next_nonce(pubkey).1;
        self.create_signed_transaction_with(secret, tx_builder, pubkey, nonce)
    }

    fn create_signed_transaction_with<T: Into<TxBuilder<App::Message>>>(
        &self,
        secret: &SecretKey,
        tx_builder: T,
        pubkey: PublicKey,
        nonce: AccountNonce,
    ) -> Result<SignedTransaction<App::Message>> {
        let TxBuilder {
            messages,
            max_height,
        } = tx_builder.into();
        let tx = Transaction::<App::Message> {
            pubkey,
            nonce,
            created: Timestamp::now(),
            messages,
            max_height,
        };
        tx.sign(secret)
    }

    /// Get the next nonce to be used for the account associated with this public key.
    ///
    /// This function is read-only, and works for both accounts that do and don't exist.
    ///
    /// For new accounts, it will always return the initial nonce.
    pub fn get_next_nonce(&self, key: PublicKey) -> (Option<AccountId>, AccountNonce) {
        self.get_framework_state()
            .accounts
            .get_account_for_key(key)
            .map_or_else(
                || (None, AccountNonce::start()),
                |(account_id, account)| (Some(account_id), account.get_next_nonce()),
            )
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

    pub fn get_chain_version(&self) -> &String {
        self.get_framework_state().get_chain_version()
    }
}
