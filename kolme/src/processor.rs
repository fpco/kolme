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
        if self.kolme.read().await.get_next_event_height().is_start() {
            tracing::info!("Creating genesis event");
            self.create_genesis_event().await?;
        }

        // Subscribe to any newly arrived events.
        let mut receiver = self.kolme.subscribe();

        // Now that we're subscribed, do an initial catch-up for any events
        // that were in the database but not yet executed.
        self.catch_up_exec_state().await?;
        tracing::info!("Processor has caught up, waiting for new events.");

        loop {
            let notification = receiver.recv().await?;
            match notification {
                Notification::NewEvent(_event_height) => {
                    // Any time a new event lands, make sure we're fully synchronized
                    // up to the latest event. This avoids any race conditions from events
                    // added between our previous catch-up and subscribing to the channel.
                    self.catch_up_exec_state().await?;
                }
                Notification::NewExec(_) => continue,
                Notification::GenesisInstantiation { chain, contract } => {
                    // FIXME In the future, we need the listeners to pick this up, validate, and send to the processor for final checking. For now, taking a shortcut and simply trusting the message.
                    let pubkey = self.secret.public_key();
                    let nonce = self.kolme.read().await.get_next_account_nonce(pubkey);
                    let payload = EventPayload::<App::Message> {
                        pubkey,
                        nonce,
                        created: Timestamp::now(),
                        messages: vec![EventMessage::BridgeCreated(BridgeCreated {
                            chain,
                            contract,
                        })],
                    };
                    self.propose(ProposedEvent(TaggedJson::new(payload)?.sign(&self.secret)?))
                        .await?;
                }
            }
        }
    }

    async fn catch_up_exec_state(&self) -> Result<()> {
        while self.kolme.read().await.get_next_event_height()
            > self.kolme.read().await.get_next_exec_height()
        {
            tracing::info!(
                "Calculating exec state for height {}",
                self.kolme.read().await.get_next_exec_height()
            );
            self.produce_next_state().await?;
        }
        Ok(())
    }

    pub async fn create_genesis_event(&self) -> Result<()> {
        let payload = EventPayload {
            pubkey: self.secret.public_key(),
            nonce: self
                .kolme
                .read()
                .await
                .get_next_account_nonce(self.secret.public_key()),
            messages: vec![EventMessage::<App::Message>::Genesis(App::genesis_info())],
            created: Timestamp::now(),
        };
        let proposed = payload.sign(&self.secret)?;
        self.propose(proposed).await?;
        Ok(())
    }

    pub async fn produce_next_state(&self) -> Result<()> {
        let mut kolme = self.kolme.write().await.begin_db_transaction().await?;
        let next_height = kolme.get_next_exec_height();
        anyhow::ensure!(kolme.get_next_event_height() > next_height);
        let ApprovedEvent::<App::Message> {
            event,
            timestamp: _,
            processor: _,
            height,
            parent: _,
        } = kolme.load_event(next_height).await?.0.message.into_inner();
        anyhow::ensure!(height == next_height);
        let EventPayload {
            pubkey: _,
            nonce: _,
            created: _,
            messages,
        } = event.0.message.into_inner();

        match kolme.execute_messages(&messages).await {
            Ok(outputs) => {
                assert_eq!(outputs.len(), messages.len());
                kolme
                    .save_execution_state(next_height, outputs, &self.secret)
                    .await?;
            }
            Err(e) => {
                todo!("Implement a rollback: {e}")
            }
        }
        Ok(())
    }

    pub async fn propose(&self, event: ProposedEvent<App::Message>) -> Result<()> {
        // Ensure that the signature is valid
        event.validate_signature()?;

        // Take a write lock to ensure nothing else tries to mutate the database at the same time.
        let kolme = self.kolme.write().await;

        // Do read-only validation of the data and sign the event.
        let signed_event = {
            let kolme = kolme.downgrade();

            // Make sure the nonce is correct
            let expected_nonce = kolme.get_next_account_nonce(event.0.message.as_inner().pubkey);
            // FIXME should we do some verification of the account ID? Make the user submit an account ID for anything beyond the first action?
            anyhow::ensure!(expected_nonce == event.0.message.as_inner().nonce);

            // Make sure this is a genesis event if and only if we have no events so far
            let next_event_height = kolme.get_next_event_height();
            let parent_hash = kolme.get_current_event_hash();
            if kolme.get_next_event_height().is_start() {
                event.ensure_is_genesis()?;
                anyhow::ensure!(event.0.message.as_inner().pubkey == kolme.get_processor_pubkey());
            } else {
                event.ensure_no_genesis()?;
            };

            let now = Timestamp::now();
            let approved_event = ApprovedEvent {
                event,
                timestamp: now,
                processor: self.secret.public_key(),
                height: next_event_height,
                parent: *parent_hash,
            };
            let event = TaggedJson::new(approved_event)?;
            SignedEvent(event.sign(&self.secret)?)
        };

        // Now that we have a signed event, reuse the existing write lock to insert it.
        let mut kolme = kolme.begin_db_transaction().await?;
        kolme.insert_event(&signed_event).await?;
        kolme.commit().await?;
        Ok(())
    }
}
