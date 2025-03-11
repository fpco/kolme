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
        while self.kolme.get_next_event_height() > self.kolme.get_next_state_height() {
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
        let mut raw = self.kolme.inner.framework_state.write();

        // Make sure the nonce is correct
        let expected_nonce = match raw.get_account_id(&event.payload.pubkey) {
            Some(account_id) => raw.get_next_nonce(account_id)?,
            None => AccountNonce::start(),
        };
        anyhow::ensure!(expected_nonce == event.payload.nonce);

        // Make sure this is a genesis event if and only if we have no events so far
        if raw.next_event_height.is_start() {
            event.ensure_is_genesis()?;
            anyhow::ensure!(event.payload.pubkey == raw.raw.processor);
        } else {
            event.ensure_no_genesis()?;
        };

        self.insert_event(&mut raw, event).await
    }

    async fn insert_event(
        &self,
        raw: &mut FrameworkState,
        event: crate::event::ProposedEvent<App::Message>,
    ) -> Result<()> {
        // FIXME current approach causes a new FrameworkState on each event due to nonce updates. May want to consider removing nonce management from that state.
        // Looks like we may need an event stream state, a framework state, and an app state.

        let now = Timestamp::now();
        let height = raw.next_event_height;
        raw.next_event_height = raw.next_event_height.next();
        let account_id = raw.get_or_insert_account_id(&event.payload.pubkey, now);
        todo!()
    }
}
