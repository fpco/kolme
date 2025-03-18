use crate::core::*;

/// Execution context for a single message.
pub struct ExecutionContext<App: KolmeApp> {
    framework_state: FrameworkState,
    app_state: App::State,
    output: MessageOutput,
    /// If we're doing a validation run, these are the prior data loads.
    validation_data_loads: Option<Vec<BlockDataLoad>>,
    /// Who signed the transaction
    pubkey: PublicKey,
    pool: sqlx::SqlitePool,
}

pub struct ExecutionResults<App: KolmeApp> {
    pub framework_state: FrameworkState,
    pub app_state: App::State,
    pub outputs: Vec<MessageOutput>,
}

impl<App: KolmeApp> KolmeInner<App> {
    /// Provide the validation data loads if we're doing a validation of a block.
    pub async fn execute_messages(
        &self,
        tx: &Transaction<App::Message>,
        validation_data_loads: Option<Vec<BlockDataLoad>>,
    ) -> Result<ExecutionResults<App>> {
        let mut outputs = vec![];
        let mut execution_context = ExecutionContext::<App> {
            framework_state: self.framework_state.clone(),
            app_state: self.app_state.clone(),
            output: MessageOutput::default(),
            validation_data_loads,
            pubkey: tx.pubkey,
            pool: self.pool.clone(),
        };
        for message in &tx.messages {
            execution_context.execute_message(message).await?;
            let output = std::mem::take(&mut execution_context.output);
            outputs.push(output);
        }
        if let Some(loads) = execution_context.validation_data_loads {
            // For a proper validation, every piece of data loaded during execution
            // must be used during validation.
            anyhow::ensure!(loads.is_empty());
        }
        Ok(ExecutionResults {
            framework_state: execution_context.framework_state,
            app_state: execution_context.app_state,
            outputs,
        })
    }
}

impl<App: KolmeApp> ExecutionContext<App> {
    async fn execute_message(&mut self, message: &Message<App::Message>) -> Result<()> {
        match message {
            Message::Genesis(actual) => {
                let expected = App::genesis_info();
                anyhow::ensure!(&expected == actual);
            }
            Message::App(_) => todo!(),
            Message::Listener {
                chain,
                event,
                event_id,
            } => {
                self.listener(*chain, event, *event_id).await?;
            }
            Message::Auth(_auth_message) => todo!(),
        }
        Ok(())
    }

    async fn listener(
        &mut self,
        chain: ExternalChain,
        event: &BridgeEvent,
        event_id: BridgeEventId,
    ) -> Result<()> {
        anyhow::ensure!(self.framework_state.listeners.contains(&self.pubkey));
        anyhow::ensure!(!has_already_listened(&self.pool, chain, event_id, &self.pubkey).await?);
        // FIXME do we want to ensure that the event hasn't been accepted yet?
        // FIXME should we include a requirement that events are added in order, and if a previous event hasn't been accepted yet, we disallow it being added here?
        ensure_event_matches(&self.pool, chain, event_id, event).await?;

        // OK, valid event. Let's find out how many existing signatures there are so we can decide if we can execute.
        let existing_signatures = count_listener_signatures(&self.pool, chain, event_id).await?;
        if existing_signatures + 1 >= self.framework_state.needed_listeners {
            todo!("execute the listener event");
        }
        Ok(())
    }
}

async fn has_already_listened(
    pool: &sqlx::SqlitePool,
    chain: ExternalChain,
    event_id: BridgeEventId,
    pubkey: &PublicKey,
) -> Result<bool> {
    let chain = chain.as_ref();
    let pubkey = pubkey.to_sec1_bytes();
    let event_id = i64::try_from(event_id.0)?;
    let count = sqlx::query_scalar!(
        r#"
            SELECT COUNT(*)
            FROM bridge_events
            INNER JOIN bridge_event_attestations
            ON bridge_events.id=bridge_event_attestations.event
            WHERE chain=$1
            AND event_id=$2
            AND public_key=$3
        "#,
        chain,
        event_id,
        pubkey
    )
    .fetch_one(pool)
    .await?;
    assert!(count == 0 || count == 1);
    Ok(count == 1)
}

async fn count_listener_signatures(
    pool: &sqlx::SqlitePool,
    chain: ExternalChain,
    event_id: BridgeEventId,
) -> Result<usize> {
    let chain = chain.as_ref();
    let event_id = i64::try_from(event_id.0)?;
    let count = sqlx::query_scalar!(
        r#"
            SELECT COUNT(*)
            FROM bridge_events
            INNER JOIN bridge_event_attestations
            ON bridge_events.id=bridge_event_attestations.event
            WHERE chain=$1
            AND event_id=$2
        "#,
        chain,
        event_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(count.try_into()?)
}

async fn ensure_event_matches(
    pool: &sqlx::SqlitePool,
    chain: ExternalChain,
    event_id: BridgeEventId,
    event: &BridgeEvent,
) -> Result<()> {
    let chain = chain.as_ref();
    let event_id = i64::try_from(event_id.0)?;
    let existing = sqlx::query_scalar!(
        r#"
            SELECT event
            FROM bridge_events
            WHERE chain=$1
            AND event_id=$2
        "#,
        chain,
        event_id,
    )
    .fetch_optional(pool)
    .await?;
    if let Some(existing) = existing {
        let new = serde_json::to_string(event)?;
        anyhow::ensure!(existing == new);
    }
    Ok(())
}
