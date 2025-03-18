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
                Notification::NewBlock(block) => {
                    // We probably need to add the new block, just in case it came from another node
                    todo!()
                }
                Notification::GenesisInstantiation { chain, contract } => {
                    // FIXME In the future, we need the listeners to pick this up, validate, and send to the processor for final checking. For now, taking a shortcut and simply trusting the message.
                    let pubkey = self.secret.public_key();
                    let nonce = self
                        .kolme
                        .read()
                        .await
                        .get_next_account_nonce(pubkey)
                        .await?;
                    let payload = Transaction::<App::Message> {
                        pubkey,
                        nonce,
                        created: Timestamp::now(),
                        messages: vec![Message::BridgeCreated(BridgeCreated { chain, contract })],
                    };
                    self.add_transaction(SignedTransaction(
                        TaggedJson::new(payload)?.sign(&self.secret)?,
                    ))
                    .await?;
                }
                Notification::Broadcast { tx } => self.add_transaction(tx).await?,
            }
        }
    }

    pub async fn create_genesis_event(&self) -> Result<()> {
        let payload = Transaction {
            pubkey: self.secret.public_key(),
            nonce: self
                .kolme
                .read()
                .await
                .get_next_account_nonce(self.secret.public_key())
                .await?,
            messages: vec![Message::<App::Message>::Genesis(App::genesis_info())],
            created: Timestamp::now(),
        };
        let proposed = payload.sign(&self.secret)?;
        self.add_transaction(proposed).await?;
        Ok(())
    }

    pub async fn produce_next_state(&self) -> Result<()> {
        todo!();
        // let mut kolme = self.kolme.write().await.begin_db_transaction().await?;
        // let next_height = kolme.get_next_exec_height();
        // anyhow::ensure!(kolme.get_next_event_height() > next_height);
        // let Block::<App::Message> {
        //     event,
        //     timestamp: _,
        //     processor: _,
        //     height,
        //     parent: _,
        // } = kolme.load_block(next_height).await?.0.message.into_inner();
        // anyhow::ensure!(height == next_height);
        // let Transaction {
        //     pubkey: _,
        //     nonce: _,
        //     created: _,
        //     messages,
        // } = event.0.message.into_inner();

        // match kolme.execute_messages(&messages).await {
        //     Ok(outputs) => {
        //         assert_eq!(outputs.len(), messages.len());
        //         kolme
        //             .save_execution_state(next_height, outputs, &self.secret)
        //             .await?;
        //     }
        //     Err(e) => {
        //         todo!("Implement a rollback: {e}")
        //     }
        // }
        // Ok(())
    }

    async fn add_transaction(&self, tx: SignedTransaction<App::Message>) -> Result<()> {
        match self.construct_block(tx).await {
            Ok(block) => {
                let mut kolme = self.kolme.write().await.begin_db_transaction().await?;
                kolme.add_block(block).await?;
                kolme.commit().await?;
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
        } = kolme
            .execute_messages(&tx.0.message.as_inner().messages)
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
