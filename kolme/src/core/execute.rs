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
    db_updates: Vec<DatabaseUpdate>,
    /// Next account ID to assign out
    next_account_id: AccountId,
}

pub struct ExecutionResults<App: KolmeApp> {
    pub framework_state: FrameworkState,
    pub app_state: App::State,
    pub outputs: Vec<MessageOutput>,
    pub db_updates: Vec<DatabaseUpdate>,
}

pub enum DatabaseUpdate {
    ListenerAttestation {
        chain: ExternalChain,
        event_id: BridgeEventId,
        event_content: String,
        msg_index: usize,
        was_accepted: bool,
        /// If this is a signed action, what's the action ID?
        action_id: Option<BridgeActionId>,
    },

    AddAccount {
        id: AccountId,
    },

    AddWalletToAccount {
        id: AccountId,
        wallet: String,
    },

    AddPubkeyToAccount {
        id: AccountId,
        pubkey: PublicKey,
    },
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
            db_updates: vec![],
            // FIXME need to investigate how we know if the current transaction signer will also get a new account ID
            next_account_id: self.get_next_account_id().await?,
        };
        for (msg_index, message) in tx.messages.iter().enumerate() {
            execution_context
                .execute_message(message, msg_index)
                .await?;
            let output = std::mem::take(&mut execution_context.output);
            outputs.push(output);
        }

        let ExecutionContext {
            framework_state,
            app_state,
            output: _,
            validation_data_loads,
            pubkey: _,
            pool: _,
            db_updates,
            next_account_id: _,
        } = execution_context;

        if let Some(loads) = validation_data_loads {
            // For a proper validation, every piece of data loaded during execution
            // must be used during validation.
            anyhow::ensure!(loads.is_empty());
        }

        Ok(ExecutionResults {
            framework_state,
            app_state,
            outputs,
            db_updates,
        })
    }
}

impl<App: KolmeApp> ExecutionContext<App> {
    async fn execute_message(
        &mut self,
        message: &Message<App::Message>,
        msg_index: usize,
    ) -> Result<()> {
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
                self.listener(*chain, event, *event_id, msg_index).await?;
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
        msg_index: usize,
    ) -> Result<()> {
        anyhow::ensure!(self.framework_state.listeners.contains(&self.pubkey));
        anyhow::ensure!(!has_already_listened(&self.pool, chain, event_id, &self.pubkey).await?);
        // FIXME do we want to ensure that the event hasn't been accepted yet?
        // FIXME should we include a requirement that events are added in order, and if a previous event hasn't been accepted yet, we disallow it being added here?
        let event_content = ensure_event_matches(&self.pool, chain, event_id, event).await?;

        // OK, valid event. Let's find out how many existing signatures there are so we can decide if we can execute.
        let existing_signatures = count_listener_signatures(&self.pool, chain, event_id).await?;

        // We accept this event if the existing signatures, plus our newest signature, meet the quorum requirements.
        // FIXME do we need to check that the listeners in the database match our current set of listeners?
        let was_accepted = existing_signatures + 1 >= self.framework_state.needed_listeners;
        let mut action_id = None;
        if was_accepted {
            match event {
                BridgeEvent::Instantiated { contract } => {
                    let config = self
                        .framework_state
                        .chains
                        .get_mut(&chain)
                        .context("Found a listener event for a chain we don't care about")?;
                    match config.bridge {
                        BridgeContract::NeededCosmosBridge { code_id:_ } => (),
                        BridgeContract::Deployed(_) => anyhow::bail!("Already have a bridge contract for {chain:?}, just received another from a listener"),
                    }
                    config.bridge = BridgeContract::Deployed(contract.clone());
                }
                BridgeEvent::Regular {
                    wallet,
                    funds,
                    keys,
                } => {
                    let account_id = self.get_account_id_for_wallet(wallet).await?;
                    for key in keys {
                        self.db_updates.push(DatabaseUpdate::AddPubkeyToAccount {
                            id: account_id,
                            pubkey: *key,
                        });
                    }
                    for BridgedAssetAmount { denom, amount } in funds {
                        let Some(asset_config) = self
                            .framework_state
                            .chains
                            .get(&chain)
                            .context("Unknown chain")?
                            .assets
                            .get(&AssetName(denom.clone()))
                        else {
                            continue;
                        };

                        *self
                            .framework_state
                            .balances
                            .entry(account_id)
                            .or_default()
                            .entry(asset_config.asset_id)
                            .or_default() += amount;
                    }
                }
                BridgeEvent::Signed {
                    wallet: _,
                    action_id: action_id_tmp,
                } => {
                    // TODO in the future we may track wallet addresses that submitted signed actions to give them rewards.
                    action_id = Some(*action_id_tmp);
                }
            }
        }
        self.db_updates.push(DatabaseUpdate::ListenerAttestation {
            chain,
            event_id,
            event_content,
            msg_index,
            was_accepted,
            action_id,
        });
        Ok(())
    }

    async fn get_account_id_for_wallet(&mut self, wallet: &str) -> Result<AccountId> {
        let account_id = sqlx::query_scalar!(
            "SELECT account_id FROM account_wallets WHERE wallet=$1",
            wallet
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match account_id {
            Some(id) => AccountId(id.try_into()?),
            None => {
                let id = self.next_account_id;
                self.next_account_id = self.next_account_id.next();
                self.db_updates.push(DatabaseUpdate::AddAccount { id });
                self.db_updates.push(DatabaseUpdate::AddWalletToAccount {
                    id,
                    wallet: wallet.to_owned(),
                });
                id
            }
        })
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

/// Returns the rendered version of the event
async fn ensure_event_matches(
    pool: &sqlx::SqlitePool,
    chain: ExternalChain,
    event_id: BridgeEventId,
    event: &BridgeEvent,
) -> Result<String> {
    let new_rendered = serde_json::to_string(event)?;
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
        anyhow::ensure!(existing == new_rendered);
    }
    Ok(new_rendered)
}
