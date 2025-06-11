use std::collections::HashMap;

use kolme_store::KolmeStoreError;

use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secrets: HashMap<PublicKey, SecretKey>,
    ready: tokio::sync::watch::Sender<bool>,
    latest_block_delay: tokio::time::Duration,
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Processor {
            kolme,
            secrets: std::iter::once((secret.public_key(), secret)).collect(),
            ready: tokio::sync::watch::channel(false).0,
            latest_block_delay: tokio::time::Duration::from_secs(10),
        }
    }

    pub fn add_secret(&mut self, secret: SecretKey) {
        self.secrets.insert(secret.public_key(), secret);
    }

    pub fn set_latest_block_delay(&mut self, latest_block_delay: tokio::time::Duration) {
        self.latest_block_delay = latest_block_delay;
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

        let producer_loop = async {
            loop {
                // FIXME need to do more validation in add_block that
                // this is the right version.
                //
                // Also want to emit some notification (probably the LatestHeight one?)
                // about what the current version is, and use that in wait_for_active_version
                // as well to avoid needing to synchronize the state fully.
                self.kolme.wait_for_active_version().await;

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
        };

        let latest_loop = async {
            loop {
                if let Err(e) = self.emit_latest().await {
                    tracing::error!("Error emitting latest block: {e}");
                }

                tokio::time::sleep(self.latest_block_delay).await;
            }
        };

        tokio::join!(producer_loop, latest_loop);

        // TODO: this function shouldn't return Result
        Ok(())
    }

    async fn ensure_genesis_event(&self) -> Result<()> {
        if self.kolme.read().get_next_height().is_start() {
            let code_version = self.kolme.get_code_version();
            let kolme = self.kolme.read();
            let chain_version = kolme.get_chain_version();
            if code_version != chain_version {
                tracing::info!("Running processor with code version {code_version}, but chain is on version {chain_version}, unable to create genesis event");
                Ok(())
            } else {
                tracing::info!("Creating genesis event");
                self.create_genesis_event().await
            }
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

        let executed_block = self
            .construct_block(signed, kolme.get_next_height())
            .await?;
        if let Err(e) = self.kolme.add_executed_block(executed_block).await {
            if let Some(KolmeStoreError::BlockAlreadyInDb { height: _ }) = e.downcast_ref() {
                self.kolme.resync().await?;
            }
            Err(e)
        } else {
            self.emit_latest().await.ok();
            Ok(())
        }
    }

    async fn add_transaction(&self, tx: SignedTransaction<App::Message>) -> Result<()> {
        // We'll retry adding a transaction multiple times before giving up.
        // We only retry if the transaction is still not present in the database,
        // and our failure is because of a block creation race condition.
        let txhash = tx.hash();
        let _construct_lock = self.kolme.take_construct_lock().await?;
        self.kolme.resync().await?;
        if self.kolme.read().get_tx_height(txhash).await?.is_some() {
            return Ok(());
        }
        let proposed_height = self.kolme.read().get_next_height();
        let res = async {
            let executed_block = self.construct_block(tx.clone(), proposed_height).await?;
            self.kolme.add_executed_block(executed_block).await
        }
        .await;
        if let Err(e) = &res {
            if let Some(KolmeStoreError::BlockAlreadyInDb { height: _ }) = e.downcast_ref() {
                tracing::warn!("Unexpected BlockAlreadyInDb while adding transaction, construction lock should have prevented this");
            } else {
                tracing::warn!("Giving up on adding transaction {txhash}: {e}");
                let failed = (|| {
                    let failed = FailedTransaction {
                        txhash,
                        proposed_height,
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
                    Ok(failed) => self
                        .kolme
                        .notify(Notification::FailedTransaction(Arc::new(failed))),
                    Err(e) => {
                        tracing::error!("Unable to generate failed transaction notification: {e}")
                    }
                }
            }
        }
        self.emit_latest().await.ok();
        res
    }

    async fn construct_block(
        &self,
        tx: SignedTransaction<App::Message>,
        proposed_height: BlockHeight,
    ) -> Result<ExecutedBlock<App>> {
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
            logs,
            loads,
        } = kolme
            .execute_transaction(&tx, now, BlockDataHandling::NoPriorData)
            .await?;

        if let Some(max_height) = tx.0.message.as_inner().max_height {
            if max_height < proposed_height {
                return Err(KolmeError::PastMaxHeight {
                    txhash,
                    max_height,
                    proposed_height,
                }
                .into());
            }
        }

        let approved_block = Block {
            tx,
            timestamp: now,
            processor: secret.public_key(),
            height: proposed_height,
            parent: kolme.get_current_block_hash(),
            framework_state: kolme.get_merkle_manager().serialize(&framework_state)?.hash,
            app_state: kolme.get_merkle_manager().serialize(&app_state)?.hash,
            loads,
            logs: kolme.get_merkle_manager().serialize(&logs)?.hash,
        };
        let block = TaggedJson::new(approved_block)?;
        let signed_block = Arc::new(SignedBlock(block.sign(secret)?));
        Ok(ExecutedBlock {
            signed_block,
            framework_state,
            app_state,
            logs,
        })
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

    async fn emit_latest(&self) -> Result<()> {
        let height = self
            .kolme
            .read()
            .get_next_height()
            .prev()
            .context("Emit latest block: no blocks available")?;
        let latest = LatestBlock {
            height,
            when: jiff::Timestamp::now(),
        };
        let json = TaggedJson::new(latest)?;
        let kolme = self.kolme.read();
        let secret = self.get_correct_secret(&kolme)?;
        let signed = json.sign(secret)?;
        self.kolme
            .notify(Notification::LatestBlock(Arc::new(signed)));
        Ok(())
    }
}
