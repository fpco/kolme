use std::ops::Deref;

use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: k256::SecretKey,
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: k256::SecretKey) -> Self {
        Processor { kolme, secret }
    }

    pub async fn run(self) -> Result<()> {
        if self.kolme.read().await.get_next_height().is_start() {
            tracing::info!("Creating genesis event");
            self.create_genesis_event().await?;
        }

        // Subscribe to any newly arrived events.
        let mut receiver = self.kolme.subscribe();

        loop {
            let notification = receiver.recv().await?;
            match notification {
                Notification::NewBlock(_) => {
                    // Safe to ignore, either we generated the new block ourself,
                    // in which case it's already added, or it came from another
                    // component which is responsible for adding it.
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
        // Ensure that the signature is valid
        tx.validate_signature()?;

        // Stop any changes from happening while we're processing.
        let kolme = self.kolme.read().await;

        // Make sure the nonce is correct
        let expected_nonce = kolme
            .get_next_account_nonce(tx.0.message.as_inner().pubkey)
            .await?;
        // FIXME should we do some verification of the account ID? Make the user submit an account ID for anything beyond the first action?
        anyhow::ensure!(expected_nonce == tx.0.message.as_inner().nonce);

        // Make sure this is a genesis event if and only if we have no events so far
        let next_event_height = kolme.get_next_height();
        let parent_hash = kolme.get_current_block_hash();
        if kolme.get_next_height().is_start() {
            tx.ensure_is_genesis()?;
            anyhow::ensure!(tx.0.message.as_inner().pubkey == kolme.get_processor_pubkey());
        } else {
            tx.ensure_no_genesis()?;
        };

        let now = Timestamp::now();

        let ExecutionResults {
            framework_state,
            app_state,
            outputs,
            listener_attestations: _,
        } = kolme
            .execute_messages(tx.0.message.as_inner(), None)
            .await?;

        let framework_state = Sha256Hash::hash(serde_json::to_string(&framework_state)?);
        let app_state = Sha256Hash::hash(&App::save_state(&app_state)?);

        let approved_block = Block {
            tx,
            timestamp: now,
            processor: self.secret.public_key(),
            height: next_event_height,
            parent: parent_hash,
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
}
