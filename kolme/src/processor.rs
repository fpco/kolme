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
        if self
            .kolme
            .inner
            .state
            .read()
            .await
            .event
            .get_next_height()
            .is_start()
        {
            self.create_genesis_event().await?;
        }
        while self.kolme.inner.state.read().await.event.get_next_height()
            > self.kolme.inner.state.read().await.exec.get_next_height()
        {
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
                .inner
                .state
                .read()
                .await
                .event
                .get_next_account_nonce(self.secret.public_key()),
            messages: vec![EventMessage::<App::Message>::Genesis(App::genesis_info())],
            created: Timestamp::now(),
        };
        let proposed = payload.sign(&self.secret)?;
        self.propose(proposed).await?;
        Ok(())
    }

    pub async fn produce_next_state(&self) -> Result<()> {
        let mut guard = self.kolme.inner.state.write().await;
        let next_height = guard.exec.get_next_height();
        let next_height_i64 = next_height.try_into_i64()?;
        anyhow::ensure!(guard.event.get_next_height() > next_height);
        let query = sqlx::query_scalar!(
            r#"
            SELECT rendered
            FROM combined_stream
            WHERE height=$1
            AND NOT is_execution
            LIMIT 1
        "#,
            next_height_i64
        );
        let payload = query.fetch_one(&self.kolme.inner.pool).await?;
        let SignedEvent {
            event,
            signature: _,
        } = serde_json::from_str(&payload)?;
        let ApprovedEvent::<App::Message> {
            event:
                ProposedEvent {
                    payload:
                        EventPayload {
                            pubkey: _,
                            nonce: _,
                            created: _,
                            messages,
                        },
                    signature: _,
                },
            timestamp: _,
            processor: _,
        } = event.into_inner();

        match guard.execute_messages(&messages).await {
            Ok(outputs) => {
                assert_eq!(outputs.len(), messages.len());
                self.save_execution_state(&mut *guard, next_height, outputs)
                    .await?;
            }
            Err(e) => {
                todo!("Implement a rollback: {e}")
            }
        }
        Ok(())
    }

    async fn save_execution_state(
        &self,
        guard: &mut KolmeState<App>,
        height_orig: EventHeight,
        outputs: Vec<MessageOutput>,
    ) -> Result<()> {
        assert_eq!(guard.exec.get_next_height(), height_orig);
        let height = height_orig.try_into_i64()?;

        let mut trans = self.kolme.inner.pool.begin().await?;

        for (
            message,
            MessageOutput {
                logs,
                loads,
                actions,
            },
        ) in outputs.iter().enumerate()
        {
            let message = i64::try_from(message)?;
            for (position, log) in logs.iter().enumerate() {
                let position = i64::try_from(position)?;
                sqlx::query!(
                    "INSERT INTO execution_logs(height, message, position, payload) VALUES($1, $2, $3, $4)",
                    height,
                    message,
                    position,
                    log
                )
                .execute(&mut *trans)
                .await?;
            }
            for (position, load) in loads.iter().enumerate() {
                let position = i64::try_from(position)?;
                let load = serde_json::to_string(&load)?;
                sqlx::query!(
                    "INSERT INTO execution_loads(height, message, position, payload) VALUES($1, $2, $3, $4)",
                    height,
                    message,
                    position,
                    load
                )
                .execute(&mut *trans)
                .await?;
            }
            for (position, action) in actions.iter().enumerate() {
                let position = i64::try_from(position)?;
                let action = serde_json::to_string(&action)?;
                sqlx::query!(
                    "INSERT INTO execution_actions(height, message, position, payload) VALUES($1, $2, $3, $4)",
                    height,
                    message,
                    position,
                    action
                )
                .execute(&mut *trans)
                .await?;
            }
        }

        let framework_state = insert_state_payload(
            &mut trans,
            guard.exec.serialize_and_store_framework_state()?.as_bytes(),
        )
        .await?;
        let app_state = insert_state_payload(
            &mut trans,
            guard.exec.serialize_and_store_app_state()?.as_bytes(),
        )
        .await?;

        let now = Timestamp::now();
        let exec_value = ExecutedEvent {
            height: height_orig,
            timestamp: now,
            framework_state,
            app_state,
            loads: outputs.into_iter().flat_map(|o| o.loads).collect(),
        };
        let exec = TaggedJson::new(exec_value)?;
        let signature = SigningKey::from(self.secret.clone()).sign(exec.as_bytes());
        let signed_value = SignedExec { exec, signature };
        let signed = serde_json::to_string(&signed_value)?;
        let now = now.to_string();
        let rendered_id = sqlx::query!(
            r#"
            INSERT INTO combined_stream(height, added, is_execution, rendered)
            VALUES($1, $2, TRUE, $3)
            "#,
            height,
            now,
            signed
        )
        .execute(&mut *trans)
        .await?
        .last_insert_rowid();

        sqlx::query!(
            r#"
            INSERT INTO execution_stream(height, framework_state, app_state, rendered_id)
            VALUES($1, $2, $3, $4)
        "#,
            height,
            signed_value.exec.as_inner().framework_state,
            signed_value.exec.as_inner().app_state,
            rendered_id
        )
        .execute(&mut *trans)
        .await?;

        trans.commit().await?;

        guard.exec.increment_height();
        Ok(())
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
        let event = TaggedJson::new(approved_event)?;
        let signature = SigningKey::from(&self.secret).sign(event.as_bytes());
        let signed_event = SignedEvent { event, signature };
        let signed_event = serde_json::to_string(&signed_event)?;

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
