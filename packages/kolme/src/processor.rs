use std::collections::HashMap;

use kolme_store::KolmeStoreError;
use rand::Rng;

use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secrets: HashMap<PublicKey, SecretKey>,
    ready: tokio::sync::watch::Sender<bool>,
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Processor {
            kolme,
            secrets: std::iter::once((secret.public_key(), secret)).collect(),
            ready: tokio::sync::watch::channel(false).0,
        }
    }

    pub fn add_secret(&mut self, secret: SecretKey) {
        self.secrets.insert(secret.public_key(), secret);
    }

    pub async fn run(self) -> Result<()> {
        let chains = self
            .kolme
            .read()
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
            let tx = Arc::unwrap_or_clone(tx);

            // Remove the transaction from the mempool. Either it will succeed, in
            // which case it's been added, or it will fail, in which case we want
            // to give up.
            self.kolme.remove_from_mempool(txhash);

            match self.add_transaction(tx).await {
                Ok(()) => {
                    // TODO See https://github.com/fpco/kolme/issues/122
                    self.approve_actions_all(&chains).await;
                }
                Err(e) => {
                    tracing::error!("Unable to add transaction {txhash} from mempool: {e}");
                }
            }
        }
    }

    async fn ensure_genesis_event(&self) -> Result<()> {
        if self.kolme.read().get_next_height().is_start() {
            tracing::info!("Creating genesis event");
            self.create_genesis_event().await
        } else {
            tracing::info!("Genesis event already present");
            Ok(())
        }
    }

    /// Get the correct secret key for the current validator set.
    ///
    /// If we don't have it, returns an error.
    fn get_correct_secret(&self, kolme: &KolmeRead<App>) -> Result<&SecretKey> {
        let pubkey = &kolme.get_framework_state().get_validator_set().processor;
        self.secrets.get(pubkey).with_context(|| {
            format!(
                "Current processor pubkey is {pubkey}, but we don't have the matching secret key"
            )
        })
    }

    pub async fn create_genesis_event(&self) -> Result<()> {
        let info = self.kolme.get_app().genesis_info().clone();
        let kolme = self.kolme.read();
        let secret = self.get_correct_secret(&kolme)?;
        let signed = self
            .kolme
            .read()
            .create_signed_transaction(secret, vec![Message::<App::Message>::Genesis(info)])?;

        let block = self.construct_block(signed).await?;
        if let Err(e) = self.kolme.add_block(Arc::new(block)).await {
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
            if self.kolme.read().get_tx_height(txhash).await?.is_some() {
                break Ok(());
            }
            attempts += 1;
            let res = async {
                let _construct_lock = self.kolme.take_construct_lock().await?;
                self.kolme.resync().await?;
                let block = self.construct_block(tx.clone()).await?;
                self.kolme.add_block(Arc::new(block)).await
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
            let failed = (|| {
                let failed = FailedTransaction {
                    txhash,
                    error: match e.downcast_ref::<KolmeError>() {
                        Some(e) => e.clone(),
                        None => KolmeError::Other(e.to_string()),
                    },
                };
                let failed = TaggedJson::new(failed)?;
                let key = self.get_correct_secret(&self.kolme.read())?;
                failed.sign(key)
            })();

            match failed {
                Ok(failed) => self.kolme.notify(Notification::FailedTransaction(failed)),
                Err(e) => {
                    tracing::error!("Unable to generate failed transaction notification: {e}")
                }
            }
        }
        res
    }

    async fn construct_block(
        &self,
        tx: SignedTransaction<App::Message>,
    ) -> Result<SignedBlock<App::Message>> {
        // Stop any changes from happening while we're processing.
        let kolme = self.kolme.read();
        let secret = self.get_correct_secret(&kolme)?;

        let txhash = tx.hash();
        if kolme.get_tx_height(txhash).await?.is_some() {
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
            processor: secret.public_key(),
            height: kolme.get_next_height(),
            parent: kolme.get_current_block_hash(),
            framework_state,
            app_state,
            loads,
        };
        let event = TaggedJson::new(approved_block)?;
        Ok(SignedBlock(event.sign(secret)?))
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
        let kolme = self.kolme.read();
        let secret = self.get_correct_secret(&kolme)?;

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

        if approvers.len() < usize::from(kolme.get_needed_approvers()) {
            // Not enough approvals. Don't bother with later actions, we want to approve in order.
            return Ok(());
        }

        // Handle the key rotation case, where a previous processor already signed.
        if action.processor.is_some() {
            return Ok(());
        }

        let processor = secret.sign_recoverable(&action.payload)?;

        let tx = kolme.create_signed_transaction(
            secret,
            vec![Message::ProcessorApprove {
                chain,
                action_id,
                processor,
                approvers,
            }],
        )?;

        std::mem::drop(kolme);
        self.add_transaction(tx).await?;

        Ok(())
    }
}
