use std::ops::Deref;

use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Processor { kolme, secret }
    }

    pub async fn run(self) -> Result<()> {
        let chains = self
            .kolme
            .read()
            .await
            .get_bridge_contracts()
            .keys()
            .copied()
            .collect::<Vec<_>>();
        if self.kolme.read().await.get_next_height().is_start() {
            tracing::info!("Creating genesis event");
            self.create_genesis_event().await?;
        }

        // Subscribe to any newly arrived events.
        let mut receiver = self.kolme.subscribe();

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
        } = kolme.execute_transaction(&tx, None).await?;

        let framework_state = kolme.get_merkle_manager().serialize(&framework_state)?.hash;
        let app_state = Sha256Hash::hash(&App::save_state(&app_state)?);

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
            let key2 = sig.validate(payload.as_bytes())?;
            anyhow::ensure!(key == &key2);
            if kolme.get_approver_pubkeys().contains(key) {
                approvers.push(*sig);
            }
        }

        if approvers.len() < kolme.get_needed_approvers() {
            // Not enough approvals. Don't bother with later actions, we want to approve in order.
            return Ok(());
        }

        let (sig, recid) = self.secret.sign_recoverable(payload.as_bytes())?;
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
