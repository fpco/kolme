use std::path::PathBuf;

use sqlx::{migrate::Migrator, postgres::PgListener};
use tokio::task::JoinSet;

use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
    block_db: Option<BlockDb>,
    ready: tokio::sync::watch::Sender<bool>,
}

/// A database that stores blocks in durable storage.
///
/// Motivation is twofold:
///
/// * Ensure that any block that we produce ends up stored long-term in storage.
///
/// * If there are multiple processors, ensure that they all agree on the chain history.
///
/// Under the surface, this simply uses a PostgreSQL database.
#[derive(Clone)]
pub struct BlockDb {
    pool: sqlx::PgPool,
    resync_trigger: tokio::sync::watch::Sender<usize>,
    resync_completed: tokio::sync::watch::Sender<Option<jiff::Timestamp>>,
}

impl BlockDb {
    /// Use the given connection pool.
    pub async fn new(pool: sqlx::PgPool) -> Result<Self> {
        let migrator = Migrator::new(PathBuf::from("blockdb-migrations")).await?;
        migrator.run(&pool).await?;
        let resync_trigger = tokio::sync::watch::channel(0).0;
        let resync_completed = tokio::sync::watch::channel(None).0;
        Ok(BlockDb {
            pool,
            resync_trigger,
            resync_completed,
        })
    }

    async fn listen_new_blocks(self) {
        loop {
            match self.listen_new_blocks_inner().await {
                Ok(()) => tracing::warn!("Unexpected exit from get_new_blocks_inner"),
                Err(e) => tracing::warn!("Error from get_new_blocks_inner: {e}"),
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }

    async fn listen_new_blocks_inner(&self) -> Result<()> {
        let mut listener = PgListener::connect_with(&self.pool).await?;
        listener.listen("new_block_channel").await?;
        loop {
            let _ = listener.recv().await?;
            self.resync_trigger.send_modify(|x| *x += 1);
        }
    }

    async fn resync_loop<App: KolmeApp>(self, kolme: Kolme<App>) {
        let mut recv = self.resync_trigger.subscribe();
        loop {
            tokio::time::timeout(tokio::time::Duration::from_secs(5), recv.changed())
                .await
                .ok();
            if let Err(e) = self.resync(&kolme).await {
                tracing::error!("Unable to resync with block DB: {e:?}");
            }
        }
    }

    async fn resync<App: KolmeApp>(&self, kolme: &Kolme<App>) -> Result<()> {
        loop {
            let height = kolme.read().await.get_next_height();
            let rendered: Option<String> =
                sqlx::query_scalar("SELECT rendered FROM blocks WHERE height=$1")
                    .bind(height.try_into_i64()?)
                    .fetch_optional(&self.pool)
                    .await?;
            let Some(rendered) = rendered else {
                self.resync_completed
                    .send(Some(jiff::Timestamp::now()))
                    .ok();
                break Ok(());
            };
            let signed_block: SignedBlock<_> = serde_json::from_str(&rendered)?;
            let txhash = signed_block.0.message.as_inner().tx.hash();
            kolme
                .add_block(signed_block)
                .await
                .with_context(|| format!("Failed to resync block with txhash {txhash}"))?;
        }
    }

    pub(crate) async fn add_block<AppMessage>(
        &self,
        signed_block: &SignedBlock<AppMessage>,
    ) -> Result<()> {
        let height = signed_block.0.message.as_inner().height.try_into_i64()?;
        let rendered = serde_json::to_string(&signed_block)?;
        let blockhash = signed_block.hash();
        let txhash = signed_block.0.message.as_inner().tx.hash();
        if let Err(e) = sqlx::query(
            "INSERT INTO blocks(height,rendered,blockhash,txhash) VALUES($1, $2, $3, $4)",
        )
        .bind(height)
        .bind(&rendered)
        .bind(blockhash.to_string())
        .bind(txhash.to_string())
        .execute(&self.pool)
        .await
        {
            // TODO is there a way to do this that doesn't involve string comparisons?
            if e.to_string().contains("violates unique constraint") {
                // Check if the block is exactly identical to the one we're trying to add.
                if let Some(current) = sqlx::query_scalar::<_, String>(
                    "SELECT rendered FROM blocks WHERE height=$1 LIMIT 1",
                )
                .bind(height)
                .fetch_optional(&self.pool)
                .await?
                {
                    if current == rendered {
                        // It was the same block, so everything is OK
                        tracing::debug!("Block {height} was already present in block DB");
                        return Ok(());
                    }
                }
                Err(anyhow::Error::from(BlockDbError::BlockAlreadyInDb))
            } else {
                Err(e.into())
            }
        } else {
            Ok(())
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BlockDbError {
    #[error("Block is already present in database")]
    BlockAlreadyInDb,
    #[error("Transaction is already present in database")]
    TxAlreadyInDb,
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey, block_db: Option<BlockDb>) -> Self {
        Processor {
            kolme,
            secret,
            block_db,
            ready: tokio::sync::watch::channel(false).0,
        }
    }

    pub async fn run(self) -> Result<()> {
        match self.block_db.clone() {
            None => {
                self.run_just_processor().await;
                Err(anyhow::anyhow!("run_just_processor unexpectedly exited"))
            }
            Some(block_db) => {
                self.kolme.set_block_db(block_db.clone()).await;
                let mut set = JoinSet::new();
                set.spawn(block_db.clone().listen_new_blocks());
                set.spawn(block_db.resync_loop(self.kolme.clone()));
                set.spawn(self.run_just_processor());
                match set.join_next().await {
                    None => Err(anyhow::anyhow!(
                        "Processor::run: unexpected end of processing"
                    )),
                    Some(Err(e)) => Err(anyhow::anyhow!("Panic within processor: {e}")),
                    Some(Ok(())) => Err(anyhow::anyhow!("Unexpected end of a processor subtask")),
                }
            }
        }
    }

    pub fn ready_watcher(&self) -> tokio::sync::watch::Receiver<bool> {
        self.ready.subscribe()
    }

    async fn run_just_processor(self) {
        let chains = self
            .kolme
            .read()
            .await
            .get_bridge_contracts()
            .keys()
            .copied()
            .collect::<Vec<_>>();

        // If we're working with a block DB, wait until the first sync finishes
        // before trying to do anything.
        if let Some(block_db) = &self.block_db {
            let mut chan = block_db.resync_completed.subscribe();
            if chan.borrow_and_update().is_none() {
                chan.changed().await.ok();
            }
        }

        self.ready.send_replace(true);

        tracing::info!("Ensuring genesis event...");
        while let Err(e) = self.ensure_genesis_event().await {
            tracing::error!("Unable to ensure genesis event, sleeping and retrying: {e}");
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        tracing::info!("Finished ensuring genesis event...");

        self.approve_actions_all(&chains).await;

        loop {
            let tx = self.kolme.wait_on_mempool().await;
            let txhash = tx.hash();
            loop {
                let tx = (*tx).clone();

                if let Err(e) = self.add_transaction(tx).await {
                    if let Some(KolmeError::InvalidAddBlockHeight { .. }) = e.downcast_ref() {
                        tracing::warn!("Block height race condition when adding transaction {txhash}, retrying");
                    } else {
                        tracing::error!("Unable to add transaction from mempool: {e}");
                        break;
                    }
                }
            }
        }
    }

    async fn ensure_genesis_event(&self) -> Result<()> {
        if self.kolme.read().await.get_next_height().is_start() {
            tracing::info!("Creating genesis event");
            self.create_genesis_event().await
        } else {
            tracing::info!("Genesis event already present");
            Ok(())
        }
    }

    pub async fn create_genesis_event(&self) -> Result<()> {
        let signed = self
            .kolme
            .read()
            .await
            .create_signed_transaction(
                &self.secret,
                vec![Message::<App::Message>::Genesis(App::genesis_info())],
            )
            .await?;
        let block = self.construct_block(signed).await?;
        self.kolme.add_block(block).await?;
        Ok(())
    }

    async fn add_transaction(&self, tx: SignedTransaction<App::Message>) -> Result<()> {
        // We'll retry adding a transaction multiple times before giving up.
        // We only retry if the transaction is still not present in the database,
        // and our failure is because of a block creation race condition.
        let txhash = tx.hash();
        println!("(1)Attempting to add_transaction: {txhash}");
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 5;
        let res = loop {
            if self.kolme.read().await.get_tx(txhash).await?.is_some() {
                break Ok(());
            }
            println!("(2)Attempting to add_transaction: {txhash}");
            attempts += 1;
            match self.construct_block(tx.clone()).await {
                Ok(block) => {
                    println!("(3)Attempting to add_transaction: {txhash}");
                    self.kolme.add_block(block).await?;
                    println!("(4)Attempting to add_transaction: {txhash}");
                    break Ok(());
                }
                Err(e) => {
                    println!("(5)Attempting to add_transaction: {txhash}");
                    if attempts >= MAX_ATTEMPTS {
                        break Err(e);
                    }
                    if let Some(e) = e.downcast_ref() {
                        match e {
                            BlockDbError::BlockAlreadyInDb => {
                                tracing::warn!("Block already in DB, retrying, attempt {attempts}/{MAX_ATTEMPTS}...");
                                if let Some(block_db) = &self.block_db {
                                    block_db.resync_trigger.send_modify(|x| *x += 1);
                                }
                            }
                            BlockDbError::TxAlreadyInDb => {
                                return Ok(());
                            }
                        }
                    } else {
                        break Err(e);
                    }
                }
            }
        };
        if let Err(e) = &res {
            tracing::warn!("Giving up on adding transaction {txhash}: {e}");
            self.kolme.notify(Notification::FailedTransaction {
                txhash,
                error: e.to_string(),
            });
        }
        res
    }

    async fn construct_block(
        &self,
        tx: SignedTransaction<App::Message>,
    ) -> Result<SignedBlock<App::Message>> {
        // Stop any changes from happening while we're processing.
        let kolme = self.kolme.read().await;

        let txhash = tx.hash();
        if kolme.get_tx(txhash).await?.is_some() {
            return Err(anyhow::Error::from(BlockDbError::TxAlreadyInDb));
        }

        let now = Timestamp::now();

        let ExecutionResults {
            framework_state,
            app_state,
            outputs,
            db_updates: _,
        } = kolme.execute_transaction(&tx, now, None).await?;

        let framework_state = kolme.get_merkle_manager().serialize(&framework_state)?.hash;
        let app_state = kolme.get_merkle_manager().serialize(&app_state)?.hash;

        let approved_block = Block {
            tx,
            timestamp: now,
            processor: self.secret.public_key(),
            height: kolme.get_next_height(),
            parent: kolme.get_current_block_hash(),
            framework_state,
            app_state,
            loads: outputs
                .into_iter()
                .flat_map(|output| output.loads)
                .collect(),
        };
        let event = TaggedJson::new(approved_block)?;
        Ok(SignedBlock(event.sign(&self.secret)?))
    }

    async fn approve_actions_all(&self, chains: &[ExternalChain]) {
        for chain in chains {
            if let Err(e) = self.approve_actions(*chain).await {
                tracing::warn!("Error when approving actions for {chain}: {e}");
            }
        }
    }

    async fn approve_actions(&self, chain: ExternalChain) -> Result<()> {
        // We only need to bother approving one action at a time. Each time we
        // approve an action, it produces a new block, which will allow us to check if we
        // need to approve anything else.
        let kolme = self.kolme.read().await;

        let Some(action_id) = kolme.get_first_unapproved_action(chain).await? else {
            return Ok(());
        };

        let payload = kolme.get_action_payload(chain, action_id).await?;

        let sigs = kolme
            .get_action_approval_signatures(chain, action_id)
            .await?;
        let mut approvers = vec![];
        for (key, sig) in &sigs {
            let key2 = sig.validate(&payload)?;
            anyhow::ensure!(key == &key2);
            if kolme.get_approver_pubkeys().contains(key) {
                approvers.push(*sig);
            }
        }

        if approvers.len() < kolme.get_needed_approvers() {
            // Not enough approvals. Don't bother with later actions, we want to approve in order.
            return Ok(());
        }

        let (sig, recid) = self.secret.sign_recoverable(&payload)?;
        let processor = SignatureWithRecovery { sig, recid };

        let tx = kolme
            .create_signed_transaction(
                &self.secret,
                vec![Message::ProcessorApprove {
                    chain,
                    action_id,
                    processor,
                    approvers,
                }],
            )
            .await?;

        std::mem::drop(kolme);
        self.add_transaction(tx).await?;

        Ok(())
    }
}
