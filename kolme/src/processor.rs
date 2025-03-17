use crate::*;

pub struct Processor<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: k256::SecretKey,
}

impl<App: KolmeApp> Processor<App> {
    pub fn new(kolme: Kolme<App>, secret: k256::SecretKey) -> Self {
        Processor { kolme, secret }
    }

    pub async fn run_processor(self) -> Result<()> {
        let kolme = self.kolme.read().await;
        if kolme.get_next_event_height().is_start() {
            self.create_genesis_event().await?;
        }
        while kolme.get_next_event_height() > kolme.get_next_exec_height() {
            self.produce_next_state().await?;
        }
        Err(anyhow::anyhow!(
            "Need to figure out how to run the processor here exactly... axum + outgoing listener?"
        ))
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
        } = kolme.load_event(next_height).await?.0.message.into_inner();
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
        let kolme = self.kolme.write().await.begin_db_transaction().await?;

        // Make sure the nonce is correct
        let expected_nonce = kolme.get_next_account_nonce(event.0.message.as_inner().pubkey);
        anyhow::ensure!(expected_nonce == event.0.message.as_inner().nonce);

        // Make sure this is a genesis event if and only if we have no events so far
        if kolme.get_next_event_height().is_start() {
            event.ensure_is_genesis()?;
            anyhow::ensure!(event.0.message.as_inner().pubkey == kolme.get_processor_pubkey());
        } else {
            event.ensure_no_genesis()?;
        };

        self.insert_event(kolme, event).await?;
        Ok(())
    }

    async fn insert_event(
        &self,
        mut kolme: KolmeWriteDb<App>,
        event: ProposedEvent<App::Message>,
    ) -> Result<KolmeWrite<App>> {
        let height = kolme.get_next_event_height();
        kolme.increment_event_height();
        let account_id = kolme.get_or_insert_account_id(&event.0.message.as_inner().pubkey, height);
        // TODO: review this code more carefully, do we need to check nonces more explicitly? Can we make a higher-level abstraction to avoid exposing too many internals to users of Kolme?
        kolme.bump_nonce_for(account_id)?;
        let now = Timestamp::now();
        let approved_event = ApprovedEvent {
            event,
            timestamp: now,
            processor: self.secret.public_key(),
        };
        let event = TaggedJson::new(approved_event)?;
        let signed_event = SignedEvent(event.sign(&self.secret)?);

        let height = height.try_into_i64()?;
        let now = now.to_string();
        kolme
            .add_event_to_combined(height, now, &signed_event)
            .await
    }
}
