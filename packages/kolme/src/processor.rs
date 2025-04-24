use kolme_store::KolmeStoreError;
use rand::Rng;

use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
    ready: tokio::sync::watch::Sender<bool>,
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Processor {
            kolme,
            secret,
            ready: tokio::sync::watch::channel(false).0,
        }
    }

    pub async fn run(self) -> Result<()> {
        let chains = self
            .kolme
            .read()
            .await
            .get_bridge_contracts()
            .iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>();

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
                        tracing::debug!("Block height race condition when adding transaction {txhash}, retrying");
                        //FIXME
                        // } else if let Some(BlockDbError::BlockAlreadyInDb) = e.downcast_ref() {
                        //     // TODO should we unify the different ways of detecting that a block is already in the database?
                        //     tracing::debug!("Block height race condition when adding transaction {txhash}, retrying");
                    } else {
                        tracing::error!("Unable to add transaction {txhash} from mempool: {e}");
                        break;
                    }
                } else {
                    // TODO See https://github.com/fpco/kolme/issues/122
                    self.approve_actions_all(&chains).await;
                    break;
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
        if let Err(e) = self.kolme.add_block(block).await {
            if let Some(KolmeStoreError::BlockAlreadyInDb { height: _ }) = e.downcast_ref() {
                self.kolme.resync().await?;
            }
            Err(e)
        } else {
            Ok(())
        }
    }

    async fn add_transaction(&self, tx: SignedTransaction<App::Message>) -> Result<()> {
        // We'll retry adding a transaction multiple times before giving up.
        // We only retry if the transaction is still not present in the database,
        // and our failure is because of a block creation race condition.
        let txhash = tx.hash();
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 50;
        let res = loop {
            if self.kolme.read().await.get_tx(txhash).await?.is_some() {
                break Ok(());
            }
            attempts += 1;
            let res = async {
                let block = self.construct_block(tx.clone()).await?;
                self.kolme.add_block(block).await
            }
            .await;
            match res {
                Ok(()) => {
                    break Ok(());
                }
                Err(e) => {
                    if attempts >= MAX_ATTEMPTS {
                        break Err(e);
                    }
                    if let Some(KolmeStoreError::BlockAlreadyInDb { height }) = e.downcast_ref() {
                        if let Err(e) = self.kolme.resync().await {
                            tracing::error!("Error while resyncing with database: {e}");
                        }
                        tracing::warn!("Block {height} already in DB, retrying, attempt {attempts}/{MAX_ATTEMPTS}...");

                        // Introduce a random delay to help with processor contention.
                        let millis = {
                            let mut rng = rand::thread_rng();
                            rng.gen_range(200..600)
                        };
                        tokio::time::sleep(tokio::time::Duration::from_millis(millis)).await;

                        continue;
                    }
                    break Err(e);
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
            return Err(anyhow::Error::from(KolmeStoreError::TxAlreadyInDb {
                txhash: txhash.0,
            }));
        }

        let now = Timestamp::now();

        let ExecutionResults {
            framework_state,
            app_state,
            logs: _,
            loads,
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
            loads,
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

        let Some((action_id, action)) = kolme.get_next_bridge_action(chain)? else {
            return Ok(());
        };

        let mut approvers = vec![];
        for (key, sig) in &action.approvals {
            let key2 = sig.validate(action.payload.as_bytes())?;
            anyhow::ensure!(key == &key2);
            if kolme.get_approver_pubkeys().contains(key) {
                approvers.push(*sig);
            }
        }

        if approvers.len() < kolme.get_needed_approvers() {
            // Not enough approvals. Don't bother with later actions, we want to approve in order.
            return Ok(());
        }

        let processor = self.secret.sign_recoverable(&action.payload)?;

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
