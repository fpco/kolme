use std::collections::VecDeque;

use k256::ecdsa::VerifyingKey;

use crate::core::*;

/// Execution context for a single message.
pub struct ExecutionContext<'a, App: KolmeApp> {
    framework_state: FrameworkState,
    app: &'a App,
    app_state: App::State,
    output: MessageOutput,
    /// If we're doing a validation run, these are the prior data loads.
    validation_data_loads: Option<VecDeque<BlockDataLoad>>,
    /// Who signed the transaction
    pubkey: PublicKey,
    pool: sqlx::SqlitePool,
    db_updates: Vec<DatabaseUpdate>,
    /// Next account ID to assign out
    next_account_id: AccountId,
    /// ID of the account that signed the transaction
    sender: AccountId,
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
    ApproveAction {
        pubkey: PublicKey,
        signature: k256::ecdsa::Signature,
        recovery: k256::ecdsa::RecoveryId,
        msg_index: usize,
        chain: ExternalChain,
        action_id: BridgeActionId,
    },
    ProcessorApproveAction {
        msg_index: usize,
        chain: ExternalChain,
        action_id: BridgeActionId,
    },
}

struct ValidateTxResponse {
    sender_account_id: AccountId,
    next_account_id: AccountId,
}

impl<App: KolmeApp> KolmeInner<App> {
    async fn validate_tx(
        &self,
        tx: &SignedTransaction<App::Message>,
    ) -> Result<ValidateTxResponse> {
        // Ensure that the signature is valid
        tx.validate_signature()?;

        let tx = tx.0.message.as_inner();

        // Make sure the nonce is correct
        let AccountAndNextNonce {
            id,
            exists,
            next_nonce,
        } = self.get_account_and_next_nonce(tx.pubkey).await?;
        anyhow::ensure!(next_nonce == tx.nonce);

        // Make sure this is a genesis event if and only if we have no events so far
        if self.get_next_height().is_start() {
            tx.ensure_is_genesis()?;
            anyhow::ensure!(tx.pubkey == self.get_processor_pubkey());
        } else {
            tx.ensure_no_genesis()?;
        };

        Ok(ValidateTxResponse {
            sender_account_id: id,
            next_account_id: if exists {
                self.get_next_account_id().await?
            } else {
                id.next()
            },
        })
    }

    /// Provide the validation data loads if we're doing a validation of a block.
    pub async fn execute_transaction(
        &self,
        signed_tx: &SignedTransaction<App::Message>,
        validation_data_loads: Option<Vec<BlockDataLoad>>,
    ) -> Result<ExecutionResults<App>> {
        let ValidateTxResponse {
            sender_account_id,
            next_account_id,
        } = self.validate_tx(signed_tx).await?;

        let tx = signed_tx.0.message.as_inner();

        let mut outputs = vec![];
        let mut execution_context = ExecutionContext::<App> {
            framework_state: self.framework_state.clone(),
            app_state: self.app_state.clone(),
            output: MessageOutput::default(),
            validation_data_loads: validation_data_loads.map(Into::into),
            pubkey: tx.pubkey,
            pool: self.pool.clone(),
            db_updates: vec![],
            next_account_id,
            sender: sender_account_id,
            app: &self.app,
        };
        for (msg_index, message) in tx.messages.iter().enumerate() {
            execution_context
                .execute_message(&self.app, message, msg_index)
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
            sender: _,
            app: _,
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

impl<App: KolmeApp> ExecutionContext<'_, App> {
    async fn execute_message(
        &mut self,
        app: &App,
        message: &Message<App::Message>,
        msg_index: usize,
    ) -> Result<()> {
        match message {
            Message::Genesis(actual) => {
                let expected = App::genesis_info();
                anyhow::ensure!(&expected == actual);
            }
            Message::App(msg) => {
                app.execute(self, msg).await?;
            }
            Message::Listener {
                chain,
                event,
                event_id,
            } => {
                self.listener(*chain, event, *event_id, msg_index).await?;
            }
            Message::Approve {
                chain,
                action_id,
                signature,
                recovery,
            } => {
                self.approve(*chain, *action_id, *signature, *recovery, msg_index)
                    .await?
            }
            Message::ProcessorApprove {
                chain,
                action_id,
                processor,
                executors,
            } => {
                self.processor_approve(*chain, *action_id, processor, executors, msg_index)
                    .await?;
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

    async fn approve(
        &mut self,
        chain: ExternalChain,
        action_id: BridgeActionId,
        signature: k256::ecdsa::Signature,
        recovery: k256::ecdsa::RecoveryId,
        msg_index: usize,
    ) -> Result<()> {
        let payload = get_action_payload(&self.pool, chain, action_id).await?;
        let key = VerifyingKey::recover_from_msg(payload.as_bytes(), &signature, recovery)?;
        let key = PublicKey(key.into());
        anyhow::ensure!(self.framework_state.executors.contains(&key));
        let chain_db = chain.as_ref();
        let action_id_db = i64::try_from(action_id.0)?;
        let count = sqlx::query_scalar!(
            r#"
                SELECT COUNT(*)
                FROM actions
                INNER JOIN action_approvals
                ON actions.id=action_approvals.action
                WHERE chain=$1
                AND action_id=$2
                AND public_key=$3
            "#,
            chain_db,
            action_id_db,
            key
        )
        .fetch_one(&self.pool)
        .await?;
        anyhow::ensure!(count == 0);
        self.db_updates.push(DatabaseUpdate::ApproveAction {
            pubkey: key,
            signature,
            recovery,
            msg_index,
            chain,
            action_id,
        });
        Ok(())
    }

    async fn processor_approve(
        &mut self,
        chain: ExternalChain,
        action_id: BridgeActionId,
        processor: &SignatureWithRecovery,
        executors: &[SignatureWithRecovery],
        msg_index: usize,
    ) -> Result<()> {
        anyhow::ensure!(executors.len() >= self.framework_state.needed_executors);

        let payload = get_action_payload(&self.pool, chain, action_id).await?;

        let processor = processor.validate(payload.as_bytes())?;
        anyhow::ensure!(processor == self.framework_state.processor);

        let executors_checked = executors
            .iter()
            .map(|sig| {
                let pubkey = sig.validate(payload.as_bytes())?;
                anyhow::ensure!(self.framework_state.executors.contains(&pubkey));
                Ok(pubkey)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        anyhow::ensure!(executors.len() == executors_checked.len());

        self.db_updates
            .push(DatabaseUpdate::ProcessorApproveAction {
                msg_index,
                chain,
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

    pub fn state_mut(&mut self) -> &mut App::State {
        &mut self.app_state
    }

    pub fn get_sender_id(&self) -> AccountId {
        self.sender
    }

    pub fn withdraw_asset(
        &mut self,
        asset_id: AssetId,
        chain: ExternalChain,
        source: AccountId,
        wallet: String,
        amount: u128,
    ) -> Result<()> {
        let assets = self
            .framework_state
            .balances
            .get_mut(&source)
            .with_context(|| format!("No balances found for account ID {source}"))?;
        let balance = assets.get_mut(&asset_id).with_context(|| {
            format!("Account {source} does not have any balances for asset {asset_id}")
        })?;
        anyhow::ensure!(*balance >= amount, "Account {source} has insufficient balance of {asset_id}. Requested: {amount}. Balance: {balance}");

        if *balance == amount {
            assets.remove(&asset_id);
            if assets.is_empty() {
                self.framework_state.balances.remove(&source);
            }
        } else {
            *balance -= amount;
        }
        self.output.actions.push(ExecAction::Transfer {
            chain,
            recipient: wallet,
            funds: vec![AssetAmount {
                id: asset_id,
                amount,
            }],
        });
        Ok(())
    }

    pub async fn load_data<Req: KolmeDataRequest<App>>(
        &mut self,
        req: Req,
    ) -> Result<Req::Response> {
        let request_str = serde_json::to_string(&req)?;
        let res = match self.validation_data_loads.as_mut() {
            Some(loads) => {
                let BlockDataLoad { request, response } = loads
                    .pop_front()
                    .context("Incorrect number of data loads")?;
                let prev_req = serde_json::from_str::<Req>(&request)?;
                let prev_res = serde_json::from_str(&response)?;
                anyhow::ensure!(prev_req == req);
                req.validate(self.app, &prev_res).await?;

                prev_res
            }
            None => req.load(self.app).await?,
        };
        self.output.loads.push(BlockDataLoad {
            request: request_str,
            response: serde_json::to_string(&res)?,
        });
        Ok(res)
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        self.output.logs.push(msg.into());
    }
}

async fn has_already_listened(
    pool: &sqlx::SqlitePool,
    chain: ExternalChain,
    event_id: BridgeEventId,
    pubkey: &PublicKey,
) -> Result<bool> {
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
