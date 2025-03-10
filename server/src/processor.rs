use crate::{
    event::{EventMessage, EventPayload},
    prelude::*,
};

pub struct Processor<App> {
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
            messages: vec![EventMessage::<App::Message>::Genesis(
                App::initial_framework_state(),
            )],
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
        // Take a write lock to ensure nothing else tries to mutate the database at the same time.
        let raw = self.kolme.inner.framework_state.write();
        // Ensure that the signature is valid

        // Make sure the nonce is correct

        // Make sure this is a genesis event if and only if we have no events so far
        todo!()
    }
}
