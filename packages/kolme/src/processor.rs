use std::{ops::Deref, path::PathBuf};

use sqlx::{migrate::Migrator, postgres::PgListener};
use tokio::task::JoinSet;

use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
    block_db: Option<BlockDb>,
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
pub struct BlockDb(sqlx::PgPool);

impl BlockDb {
    /// Use the given connection pool.
    pub async fn new(pool: sqlx::PgPool) -> Result<Self> {
        let migrator = Migrator::new(PathBuf::from("blockdb-migrations")).await?;
        migrator.run(&pool).await?;
        Ok(BlockDb(pool))
    }

    async fn get_new_blocks<App: KolmeApp>(self, kolme: Kolme<App>) -> Result<()> {
        loop {
            match self.get_new_blocks_inner(&kolme).await {
                Ok(()) => tracing::warn!("Unexpected exit from get_new_blocks_inner"),
                Err(e) => tracing::warn!("Error from get_new_blocks_inner: {e}"),
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }

    async fn get_new_blocks_inner<App: KolmeApp>(&self, kolme: &Kolme<App>) -> Result<()> {
        let mut listener = PgListener::connect_with(&self.0).await?;
        listener.listen("new_block_channel").await?;
        self.resync(kolme).await?;
        loop {
            let _ = listener.recv().await?;
            self.resync(kolme).await?;
        }
    }

    async fn resync<App: KolmeApp>(&self, kolme: &Kolme<App>) -> Result<()> {
        loop {
            let height = kolme.read().await.get_next_height();
            let rendered: Option<String> =
                sqlx::query_scalar("SELECT rendered FROM blocks WHERE height=$1")
                    .bind(height.try_into_i64()?)
                    .fetch_optional(&self.0)
                    .await?;
            let Some(rendered) = rendered else {
                break Ok(());
            };
            kolme.add_block(serde_json::from_str(&rendered)?).await?;
        }
    }

    pub(crate) async fn add_block<AppMessage>(
        &self,
        signed_block: &SignedBlock<AppMessage>,
    ) -> Result<()> {
        sqlx::query("INSERT INTO blocks(height,rendered) VALUES($1, $2)")
            .bind(signed_block.0.message.as_inner().height.try_into_i64()?)
            .bind(serde_json::to_string(&signed_block)?)
            .execute(&self.0)
            .await?;
        Ok(())
    }
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey, block_db: Option<BlockDb>) -> Self {
        Processor {
            kolme,
            secret,
            block_db,
        }
    }

    pub async fn run(self) -> Result<()> {
        match self.block_db.clone() {
            None => self.run_just_processor().await,
            Some(block_db) => {
                self.kolme.set_block_db(block_db.clone()).await;
                let mut set = JoinSet::new();
                set.spawn(block_db.get_new_blocks(self.kolme.clone()));
                set.spawn(self.run_just_processor());
                while let Some(res) = set.join_next().await {
                    match res {
                        Err(e) => return Err(e.into()),
                        Ok(Err(e)) => return Err(e),
                        Ok(Ok(())) => (),
                    }
                }
                Ok(())
            }
        }
    }

    async fn run_just_processor(self) -> Result<()> {
        let chains = self
            .kolme
            .read()
            .await
            .get_bridge_contracts()
            .keys()
            .copied()
            .collect::<Vec<_>>();

        // Subscribe to any newly arrived events.
        // Do this before initialization so that any events that get sent
        // while setting up the genesis event don't get dropped.
        let mut receiver = self.kolme.subscribe();

        while let Err(e) = self.ensure_genesis_event().await {
            tracing::error!("Unable to ensure genesis event, sleeping and retrying: {e}");
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        self.approve_actions_all(&chains).await?;

        loop {
            let notification = receiver.recv().await?;
            match notification {
                Notification::NewBlock(_) => {
                    self.approve_actions_all(&chains).await?;
                }
                Notification::GenesisInstantiation {
                    chain: _,
                    contract: _,
                } => (),
                Notification::Broadcast { tx } => {
                    let tx = Arc::try_unwrap(tx).unwrap_or_else(|tx| tx.deref().clone());
                    self.add_transaction(tx).await?
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
        match self.construct_block(tx).await {
            Ok(block) => {
                self.kolme.add_block(block).await?;
                Ok(())
            }
            Err(e) => {
                tracing::error!("Error when constructing a block from a transaction. FIXME determine what to do in the future, this can happen legitimately: {e}");
                Ok(())
            }
        }
    }

    async fn construct_block(
        &self,
        tx: SignedTransaction<App::Message>,
    ) -> Result<SignedBlock<App::Message>> {
        // Stop any changes from happening while we're processing.
        let kolme = self.kolme.read().await;

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

    async fn approve_actions_all(&self, chains: &[ExternalChain]) -> Result<()> {
        for chain in chains {
            self.approve_actions(*chain).await?;
        }
        Ok(())
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
