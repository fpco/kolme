use k256::ecdsa::{signature::SignerMut, SigningKey};

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
        if self.kolme.get_next_event_height().await.is_start() {
            self.create_genesis_event().await?;
        }
        while self.kolme.get_next_event_height().await > self.kolme.get_next_exec_height().await {
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
                .get_next_account_nonce(self.secret.public_key())
                .await,
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
        let mut guard = self.kolme.inner.state.write().await;

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
        let height = state.event.get_next_height();
        state.event.increment_height();
        // FIXME finish whatever work needs to happen with account_id
        let account_id = state
            .event
            .get_or_insert_account_id(&event.payload.pubkey, height);
        state.event.bump_nonce_for(account_id)?;
        let now = Timestamp::now();
        let approved_event = ApprovedEvent {
            event,
            timestamp: now,
            processor: self.secret.public_key(),
        };
        let event_bytes = serde_json::to_vec(&approved_event)?;
        let signature = SigningKey::from(&self.secret).sign(&event_bytes);
        let signed_event = SignedEvent {
            event: event_bytes,
            signature,
        };
        let signed_event = serde_json::to_vec(&signed_event)?;

        let mut trans = self.kolme.inner.pool.begin().await?;
        let height = height.try_into_i64()?;
        let now = now.to_string();
        let combined_id = sqlx::query!("INSERT INTO combined_stream(height,added,is_execution,rendered) VALUES($1,$2,FALSE,$3)", height, now, signed_event).execute(&mut *trans).await?.last_insert_rowid();
        let event_state = state.event.serialize_raw_state()?;
        let event_state = insert_state_payload(&mut trans, &event_state).await?;
        sqlx::query!(
            "INSERT INTO event_stream(height,state,rendered_id) VALUES($1,$2,$3)",
            height,
            event_state,
            combined_id
        )
        .execute(&mut *trans)
        .await?;
        trans.commit().await?;
        Ok(())
    }
}
