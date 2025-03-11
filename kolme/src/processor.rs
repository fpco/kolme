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
        if self.kolme.get_next_event_height().is_start() {
            self.create_genesis_event().await?;
        }
        while self.kolme.get_next_event_height() > self.kolme.get_next_exec_height() {
            self.produce_next_state().await?;
        }
        Err(anyhow::anyhow!(
            "Need to figure out how to run the processor here exactly... axum + outgoing listener?"
        ))
    }

    pub async fn create_genesis_event(&self) -> Result<()> {
        let payload = EventPayload {
            pubkey: self.secret.public_key(),
            nonce: self.kolme.get_next_account_nonce(self.secret.public_key()),
            messages: vec![EventMessage::<App::Message>::Genesis(App::genesis_info())],
            created: Timestamp::now(),
        };
        let proposed = payload.sign(&self.secret)?;
        self.propose(proposed).await?;
        Ok(())
    }

    pub async fn produce_next_state(&self) -> Result<()> {
        todo!()
    }

    pub async fn propose(&self, event: crate::event::ProposedEvent<App::Message>) -> Result<()> {
        // Ensure that the signature is valid
        event.validate_signature()?;

        // Take a write lock to ensure nothing else tries to mutate the database at the same time.
        let mut guard = self.kolme.inner.state.write();

        // Make sure the nonce is correct
        let expected_nonce = match guard.event.get_account_id(&event.payload.pubkey) {
            Some(account_id) => guard.event.get_next_nonce(account_id)?,
            None => AccountNonce::start(),
        };
        anyhow::ensure!(expected_nonce == event.payload.nonce);

        // Make sure this is a genesis event if and only if we have no events so far
        if guard.event.get_next_height().is_start() {
            event.ensure_is_genesis()?;
            anyhow::ensure!(event.payload.pubkey == guard.exec.get_processor_pubkey());
        } else {
            event.ensure_no_genesis()?;
        };

        self.insert_event(&mut guard, event).await
    }

    async fn insert_event(
        &self,
        state: &mut KolmeState<App>,
        event: crate::event::ProposedEvent<App::Message>,
    ) -> Result<()> {
        let now = Timestamp::now();
        let height = state.event.get_next_height();
        state.event.increment_height();
        let account_id = state
            .event
            .get_or_insert_account_id(&event.payload.pubkey, height);
        todo!()
    }
}
