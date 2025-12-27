use std::collections::VecDeque;

use crate::core::*;

/// Execution context for a single message.
pub struct ExecutionContext<'a, App: KolmeApp> {
    framework_state: FrameworkState,
    app: &'a App,
    app_state: App::State,
    logs: Vec<Vec<String>>,
    loads: Vec<BlockDataLoad>,
    block_data_handling: BlockDataHandling,
    /// Who signed the transaction
    pubkey: PublicKey,
    /// ID of the account that signed the transaction
    sender: AccountId,
    /// Public key used to sign the current transaction.
    ///
    /// We need to ensure an account always has an active signing ability.
    /// Therefore, we disallow removing a public key from an account
    /// when it is the signer. See references to this field to see how this is used.
    signing_key: PublicKey,
    /// Timestamp corresponding to the moment of time when processor starts
    /// executing the current transaction
    timestamp: Timestamp,
    /// The block height we are trying to produce.
    height: BlockHeight,
}

#[derive(Debug)]
pub struct ExecutionResults<App: KolmeApp> {
    pub framework_state: FrameworkState,
    pub app_state: App::State,
    /// Logs collected from each message.
    pub logs: Vec<Vec<String>>,
    pub loads: Vec<BlockDataLoad>,
    pub height: BlockHeight,
}

/// An already executed block that can be added to storage.
///
/// This is used by the processor to avoid the need to execute a transaction twice
/// during processing.
pub struct ExecutedBlock<App: KolmeApp> {
    pub signed_block: Arc<SignedBlock<App::Message>>,
    pub framework_state: FrameworkState,
    pub app_state: App::State,
    /// Logs collected from each message.
    pub logs: Vec<Vec<String>>,
}

/// Specifies how block data should be handled during transaction execution.
///
/// - `NoPriorData`: Indicates that no prior block data is available or required.
/// - `PriorData`: Indicates that prior block data is available and should be used.
///   This variant includes additional fields for managing data loads and validation.
pub enum BlockDataHandling {
    /// No prior block data is available or required for this transaction.
    NoPriorData,
    /// Prior block data is available and should be used during execution.
    ///
    /// - `loads`: A queue of data loads that were retrieved from prior blocks.
    /// - `validation`: Specifies the validation rules to apply to the loaded data.
    PriorData {
        /// A queue of data loads retrieved from prior blocks.
        loads: VecDeque<BlockDataLoad>,
        /// Validation rules to ensure the correctness of the loaded data.
        validation: DataLoadValidation,
    },
}

impl<App: KolmeApp> KolmeRead<App> {
    fn validate_tx(&self, tx: &SignedTransaction<App::Message>) -> Result<()> {
        // Ensure that the signature is valid
        tx.validate_signature()?;

        let tx = tx.0.message.as_inner();

        // Make sure this is a genesis event if and only if we have no events so far
        if self.get_next_height().is_start() {
            tx.ensure_is_genesis()?;
            anyhow::ensure!(tx.pubkey == self.get_processor_pubkey());
        } else {
            tx.ensure_no_genesis()?;
        };

        Ok(())
    }

    /// Provide the validation data loads if we're doing a validation of a block.
    pub async fn execute_transaction(
        &self,
        signed_tx: &SignedTransaction<App::Message>,
        timestamp: Timestamp,
        block_data_handling: BlockDataHandling,
    ) -> Result<ExecutionResults<App>> {
        // If we're running different code versions, we can't
        // get reproducible results.
        let chain_version = self.get_chain_version();
        let code_version = self.get_code_version();
        anyhow::ensure!(chain_version == code_version, "Cannot execute transaction {}, current code version is {code_version}, but chain is running {chain_version}", signed_tx.hash());

        self.validate_tx(signed_tx)?;
        let tx = signed_tx.0.message.as_inner();

        let mut framework_state = self.get_framework_state().clone();
        let sender_account_id = framework_state.accounts.use_nonce(tx.pubkey, tx.nonce)?;

        let mut execution_context = ExecutionContext::<App> {
            framework_state,
            app_state: self.get_app_state().clone(),
            block_data_handling,
            pubkey: tx.pubkey,
            sender: sender_account_id,
            app: self.get_app(),
            signing_key: signed_tx.0.message.as_inner().pubkey,
            timestamp,
            logs: vec![],
            loads: vec![],
            height: self.get_next_height(),
        };
        for message in &tx.messages {
            execution_context.logs.push(vec![]);
            execution_context
                .execute_message(self.get_app(), message)
                .await?;
        }

        let ExecutionContext {
            framework_state,
            app_state,
            block_data_handling,
            pubkey: _,
            sender: _,
            app: _,
            signing_key: _,
            timestamp: _,
            logs,
            loads,
            height,
        } = execution_context;

        match block_data_handling {
            BlockDataHandling::NoPriorData => (),
            BlockDataHandling::PriorData {
                loads,
                validation: _,
            } => {
                // For a proper validation, every piece of data loaded during execution
                // must be used during validation.
                anyhow::ensure!(loads.is_empty());
            }
        }

        Ok(ExecutionResults {
            framework_state,
            app_state,
            logs,
            loads,
            height,
        })
    }
}

impl<App: KolmeApp> ExecutionContext<'_, App> {
    async fn execute_message(&mut self, app: &App, message: &Message<App::Message>) -> Result<()> {
        match message {
            Message::Genesis(actual) => {
                let expected = app.genesis_info();
                anyhow::ensure!(expected == actual);
            }
            Message::App(msg) => {
                app.execute(self, msg).await?;
            }
            Message::Listener {
                chain,
                event,
                event_id,
            } => {
                self.listener(*chain, event, *event_id)?;
            }
            Message::Approve {
                chain,
                action_id,
                signature,
            } => self.approve(*chain, *action_id, *signature)?,
            Message::ProcessorApprove {
                chain,
                action_id,
                processor,
                approvers,
            } => {
                self.processor_approve(*chain, *action_id, processor, approvers)?;
            }
            Message::Bank(bank) => self.bank(bank)?,
            Message::Auth(auth) => self.auth(auth)?,
            Message::Admin(admin) => self.admin(admin)?,
        }
        Ok(())
    }

    fn listener(
        &mut self,
        chain: ExternalChain,
        event: &BridgeEvent,
        event_id: BridgeEventId,
    ) -> Result<()> {
        anyhow::ensure!(self
            .framework_state
            .get_validator_set()
            .listeners
            .contains(&self.pubkey),
            "Received a listener message for bridge event ID {event_id} on {chain}, but provided pubkey {} is not part of the listener set {:?}",
            self.pubkey,
            self.framework_state.get_validator_set().listeners
        );

        let state = self.framework_state.chains.get_mut(chain)?;

        let attestations = match state.pending_events.get_mut(&event_id) {
            Some(pending) => {
                anyhow::ensure!(pending.event == *event);
                &mut pending.attestations
            }
            None => {
                anyhow::ensure!(event_id == state.next_event_id);
                state.next_event_id = event_id.next();
                state.pending_events.insert(
                    event_id,
                    PendingBridgeEvent {
                        event: event.clone(),
                        attestations: BTreeSet::new(),
                    },
                );
                &mut state
                    .pending_events
                    .get_mut(&event_id)
                    .unwrap()
                    .attestations
            }
        };

        let was_inserted = attestations.insert(self.pubkey);

        // Make sure it wasn't already approved
        anyhow::ensure!(was_inserted);

        // Now that we've added a signature, go through all pending events
        // in order and process them if they have sufficient attestations.
        self.process_ready_events(chain)
    }

    fn process_ready_events(&mut self, chain: ExternalChain) -> Result<()> {
        fn get_next_ready_event(
            framework_state: &FrameworkState,
            chain: ExternalChain,
        ) -> Option<BridgeEventId> {
            let (event_id, pending) = framework_state
                .chains
                .get(chain)
                .unwrap()
                .pending_events
                .iter()
                .next()?;

            // Let's find out how many existing signatures there are so we can decide if we can execute.
            let existing_signatures = u16::try_from(
                pending
                    .attestations
                    .iter()
                    .filter(|key| framework_state.get_validator_set().listeners.contains(key))
                    .count(),
            )
            .expect("Too many attestations found");

            // We accept this event if the existing signatures, plus our newest signature, meet the quorum requirements.
            let was_accepted =
                existing_signatures >= framework_state.get_validator_set().needed_listeners;

            // If this event isn't accepted yet, we simply exit. We never try to
            // process later events while an earlier one is unprocessed.
            if was_accepted {
                Some(*event_id)
            } else {
                None
            }
        }

        loop {
            let Some(event_id) = get_next_ready_event(&self.framework_state, chain) else {
                break;
            };
            let (_event_id, pending) = self
                .framework_state
                .chains
                .get_mut(chain)
                .unwrap()
                .pending_events
                .remove(&event_id)
                .unwrap();

            match &pending.event {
                BridgeEvent::Instantiated { contract } => {
                    let config = &mut self.framework_state.chains.get_mut(chain)?.config;
                    match config.bridge {
                        BridgeContract::NeededCosmosBridge { .. } |
                            BridgeContract::NeededSolanaBridge { .. } => (),
                        BridgeContract::Deployed(_) => anyhow::bail!("Already have a bridge contract for {chain:?}, just received another from a listener"),
                    }
                    config.bridge = BridgeContract::Deployed(contract.clone());
                }
                BridgeEvent::Regular {
                    wallet,
                    funds,
                    keys,
                } => {
                    let account_id = self.get_or_add_account_id_for_wallet(wallet);
                    for key in keys {
                        self.framework_state
                            .accounts
                            .add_pubkey_to_account_ignore_overlap(account_id, *key);
                    }
                    for BridgedAssetAmount { denom, amount } in funds {
                        let Some(asset_config) = self
                            .framework_state
                            .chains
                            .get(chain)?
                            .config
                            .assets
                            .get(&AssetName(denom.clone()))
                            .copied()
                        else {
                            continue;
                        };

                        let amount = asset_config.to_decimal(*amount)?;
                        self.framework_state.accounts.mint(
                            account_id,
                            asset_config.asset_id,
                            amount,
                        )?;
                        self.framework_state
                            .chains
                            .get_mut(chain)?
                            .deposit(asset_config.asset_id, amount)?;
                    }
                    self.log_event(LogEvent::ProcessedBridgeEvent(LogBridgeEvent::Regular {
                        bridge_event_id: event_id,
                        account_id,
                    }))?;
                }
                BridgeEvent::Signed { wallet, action_id } => {
                    // TODO in the future we may track wallet addresses that submitted signed actions to give them rewards.
                    let action_id = *action_id;
                    let actions = &mut self.framework_state.chains.get_mut(chain)?.pending_actions;
                    let next_action_id = actions
                        .keys()
                        .next()
                        .context("Cannot report on an action when no pending actions present")?;
                    anyhow::ensure!(*next_action_id == action_id);
                    let (old_id, _old) = actions.remove(&action_id).unwrap();
                    anyhow::ensure!(old_id == action_id);
                    self.log_event(LogEvent::ProcessedBridgeEvent(LogBridgeEvent::Signed {
                        wallet: wallet.clone(),
                        action_id,
                    }))?;
                }
            }
        }

        Ok(())
    }

    fn approve(
        &mut self,
        chain: ExternalChain,
        action_id: BridgeActionId,
        signature: SignatureWithRecovery,
    ) -> Result<()> {
        let action = self
            .framework_state
            .chains
            .get_mut(chain)?
            .pending_actions
            .get_mut(&action_id)
            .with_context(|| {
                format!("Cannot approve missing bridge action ID {action_id} for chain {chain}")
            })?;
        let key = signature.validate(action.payload.as_bytes())?;
        // Using config.as_ref() instead of framework_state.get_config to work around
        // a borrow conflict with the mutable borrow above
        anyhow::ensure!(self
            .framework_state
            .validator_set
            .as_ref()
            .approvers
            .contains(&key));
        let old = action.approvals.insert(key, signature);
        anyhow::ensure!(old.is_none(), "Cannot approve bridge action ID {action_id} for chain {chain} with already-used public key {key}");
        Ok(())
    }

    fn processor_approve(
        &mut self,
        chain: ExternalChain,
        action_id: BridgeActionId,
        processor: &SignatureWithRecovery,
        approvers: &[SignatureWithRecovery],
    ) -> Result<()> {
        anyhow::ensure!(
            approvers.len()
                >= self
                    .framework_state
                    .get_validator_set()
                    .needed_approvers
                    .into()
        );

        let action = self
            .framework_state
            .chains
            .get_mut(chain)?
            .pending_actions
            .get_mut(&action_id)
            .with_context(|| format!("No pending action {action_id} found for {chain}"))?;

        anyhow::ensure!(action.processor.is_none());

        let payload = action.payload.as_bytes();
        let processor_key = processor.validate(payload)?;
        anyhow::ensure!(processor_key == self.framework_state.validator_set.as_ref().processor);

        let approvers_checked = approvers
            .iter()
            .map(|sig| {
                let pubkey = sig.validate(payload)?;
                anyhow::ensure!(self
                    .framework_state
                    .validator_set
                    .as_ref()
                    .approvers
                    .contains(&pubkey));
                Ok(pubkey)
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        anyhow::ensure!(approvers.len() == approvers_checked.len());

        action.processor = Some(*processor);

        Ok(())
    }

    pub fn get_or_add_account_id_for_key(&mut self, key: &PublicKey) -> AccountId {
        self.framework_state
            .accounts
            .get_or_add_account_for_key(key)
    }

    pub fn get_or_add_account_id_for_wallet(&mut self, wallet: &Wallet) -> AccountId {
        self.framework_state
            .accounts
            .get_or_add_account_for_wallet(wallet)
            .0
    }

    pub fn app(&self) -> &App {
        self.app
    }

    pub fn app_state(&self) -> &App::State {
        &self.app_state
    }

    pub fn app_state_mut(&mut self) -> &mut App::State {
        &mut self.app_state
    }

    /// Synonym for [Self::app_state_mut]
    pub fn state_mut(&mut self) -> &mut App::State {
        &mut self.app_state
    }

    pub fn framework_state(&self) -> &FrameworkState {
        &self.framework_state
    }

    pub fn framework_state_mut(&mut self) -> &mut FrameworkState {
        &mut self.framework_state
    }

    pub fn block_time(&self) -> Timestamp {
        self.timestamp
    }

    pub fn block_height(&self) -> BlockHeight {
        self.height
    }

    pub fn get_sender_id(&self) -> AccountId {
        self.sender
    }

    pub fn get_sender_wallets(&self) -> &BTreeSet<Wallet> {
        self.framework_state
            .accounts
            .get_wallets_for(self.sender)
            .unwrap()
    }

    pub fn get_signing_key(&self) -> PublicKey {
        self.signing_key
    }

    pub fn get_account_balances(
        &self,
        account_id: &AccountId,
    ) -> Option<&BTreeMap<AssetId, Decimal>> {
        self.framework_state.accounts.get_assets(account_id)
    }

    fn add_action(&mut self, chain: ExternalChain, action: ExecAction) -> Result<BridgeActionId> {
        let state = self.framework_state.chains.get_mut(chain)?;
        let id = state.next_action_id;
        let payload = action.to_payload(chain, &state.config, id)?;
        state.pending_actions.insert(
            id,
            PendingBridgeAction {
                payload,
                approvals: BTreeMap::new(),
                processor: None,
            },
        );
        state.next_action_id = state.next_action_id.next();
        self.log_event(LogEvent::NewBridgeAction { chain, id })?;
        Ok(id)
    }

    /// Add an action on all chains
    fn add_action_all_chains(&mut self, action: ExecAction) -> Result<()> {
        let chains = self.framework_state.chains.keys().collect::<Vec<_>>();
        for chain in chains {
            self.add_action(chain, action.clone())?;
        }

        Ok(())
    }

    /// Withdraw an asset to an external chain.
    pub fn withdraw_asset(
        &mut self,
        asset_id: AssetId,
        chain: ExternalChain,
        source: AccountId,
        wallet: &Wallet,
        amount: Decimal,
    ) -> Result<BridgeActionId> {
        let config = self.framework_state.get_asset_config(chain, asset_id)?;
        let (amount_dec, amount_u128) = config.to_u128(amount)?;
        self.framework_state
            .accounts
            .burn(source, asset_id, amount_dec)?;
        self.framework_state
            .chains
            .get_mut(chain)?
            .withdraw(asset_id, amount)?;

        self.add_action(
            chain,
            ExecAction::Transfer {
                chain,
                recipient: wallet.clone(),
                funds: vec![AssetAmount {
                    id: asset_id,
                    amount: amount_u128,
                }],
            },
        )
    }

    /// Transfer an asset to another account.
    pub fn transfer_asset(
        &mut self,
        asset_id: AssetId,
        source: AccountId,
        dest: AccountId,
        amount: Decimal,
    ) -> Result<()> {
        self.burn_asset(asset_id, source, amount)?;
        self.mint_asset(asset_id, dest, amount)?;
        Ok(())
    }

    /// Mint new tokens and assign ownership to the given account.
    pub fn mint_asset(
        &mut self,
        asset_id: AssetId,
        recipient: AccountId,
        amount: Decimal,
    ) -> Result<()> {
        self.framework_state
            .accounts
            .mint(recipient, asset_id, amount)?;
        Ok(())
    }

    /// Burn some tokens from the given account.
    ///
    /// This can be used if the application itself takes possession of some assets.
    pub fn burn_asset(
        &mut self,
        asset_id: AssetId,
        owner: AccountId,
        amount: Decimal,
    ) -> Result<()> {
        self.framework_state
            .accounts
            .burn(owner, asset_id, amount)?;
        Ok(())
    }

    pub async fn load_data<Req: KolmeDataRequest<App>>(
        &mut self,
        req: Req,
    ) -> Result<Req::Response> {
        let request_str = serde_json::to_string(&req)?;
        let res = match &mut self.block_data_handling {
            BlockDataHandling::PriorData { loads, validation } => {
                let BlockDataLoad { request, response } = loads
                    .pop_front()
                    .context("Incorrect number of data loads")?;
                let prev_req = serde_json::from_str::<Req>(&request)?;
                let prev_res = serde_json::from_str(&response)?;
                anyhow::ensure!(prev_req == req);
                match validation {
                    DataLoadValidation::ValidateDataLoads => {
                        req.validate(self.app, &prev_res).await?;
                    }
                    DataLoadValidation::TrustDataLoads => (),
                }

                prev_res
            }
            BlockDataHandling::NoPriorData => req.load(self.app).await?,
        };
        self.loads.push(BlockDataLoad {
            request: request_str,
            response: serde_json::to_string(&res)?,
        });
        Ok(res)
    }

    fn bank(&mut self, bank: &BankMessage) -> Result<()> {
        match bank {
            BankMessage::Withdraw {
                asset,
                chain,
                dest,
                amount,
            } => {
                let _bridge_action_id =
                    self.withdraw_asset(*asset, *chain, self.sender, dest, *amount)?;
            }
            BankMessage::Transfer {
                asset,
                dest,
                amount,
            } => {
                self.transfer_asset(*asset, self.sender, *dest, *amount)?;
            }
        }
        Ok(())
    }

    fn auth(&mut self, auth: &AuthMessage) -> Result<()> {
        match auth {
            AuthMessage::AddPublicKey { key } => {
                self.framework_state
                    .accounts
                    .add_pubkey_to_account_error_overlap(self.get_sender_id(), *key)?;
            }
            AuthMessage::RemovePublicKey { key } => {
                anyhow::ensure!(key != &self.signing_key, "Cannot remove public key {key} from account {} with a transaction signed by the same key", self.get_sender_id());
                self.framework_state
                    .accounts
                    .remove_pubkey_from_account(self.get_sender_id(), *key)?;
            }
            AuthMessage::AddWallet { wallet } => {
                self.framework_state
                    .accounts
                    .add_wallet_to_account(self.get_sender_id(), wallet)?;
            }
            AuthMessage::RemoveWallet { wallet } => {
                self.framework_state
                    .accounts
                    .remove_wallet_from_account(self.get_sender_id(), wallet)?;
            }
        }
        Ok(())
    }

    fn admin(&mut self, admin: &AdminMessage) -> Result<()> {
        match admin {
            AdminMessage::SelfReplace(self_replace) => {
                let signer = self_replace.verify_signature()?;
                anyhow::ensure!(signer == self.pubkey);
                fn set_helper(
                    validator_set: &mut ValidatorSet,
                    is_approver: bool,
                    sender: PublicKey,
                    replacement: PublicKey,
                ) -> Result<()> {
                    let set = if is_approver {
                        &mut validator_set.approvers
                    } else {
                        &mut validator_set.listeners
                    };
                    if !set.remove(&sender) {
                        anyhow::bail!("Signing public key {} is not a member of the {} set and cannot self-replace", sender, if is_approver {"approver"}else{"listener"});
                    }
                    set.insert(replacement);
                    Ok(())
                }

                let config = self.framework_state.validator_set.as_mut();
                let replacement = self_replace.message.as_inner().replacement;
                match self_replace.message.as_inner().validator_type {
                    ValidatorType::Processor => {
                        if config.processor == signer {
                            config.processor = replacement;
                        } else {
                            anyhow::bail!("Signing public key {} is not the current processor and cannot self-replace", self.pubkey);
                        }
                    }
                    ValidatorType::Listener => {
                        set_helper(config, false, signer, replacement)?;
                    }
                    ValidatorType::Approver => {
                        set_helper(config, true, signer, replacement)?;
                    }
                }

                self.add_action_all_chains(ExecAction::SelfReplace(self_replace.clone()))?;
            }
            AdminMessage::NewSet { validator_set } => {
                let signer = validator_set.verify_signature()?;
                anyhow::ensure!(signer == self.pubkey);
                self.framework_state
                    .validator_set
                    .as_ref()
                    .ensure_is_validator(self.pubkey)?;
                validator_set.message.as_inner().validate()?;
                self.add_admin_proposal(
                    ProposalPayload::NewSet(validator_set.message.clone()),
                    self.pubkey,
                    validator_set.signature_with_recovery(),
                )?;
                self.check_pending_proposals()?;
            }
            AdminMessage::MigrateContract(migrate) => {
                let signer = migrate.verify_signature()?;
                anyhow::ensure!(signer == self.pubkey);
                self.framework_state
                    .validator_set
                    .as_ref()
                    .ensure_is_validator(self.pubkey)?;
                self.add_admin_proposal(
                    ProposalPayload::MigrateContract(migrate.message.clone()),
                    self.pubkey,
                    migrate.signature_with_recovery(),
                )?;
                self.check_pending_proposals()?;
            }
            AdminMessage::Upgrade(upgrade) => {
                let signer = upgrade.verify_signature()?;
                anyhow::ensure!(signer == self.pubkey);
                self.framework_state
                    .validator_set
                    .as_ref()
                    .ensure_is_validator(self.pubkey)?;
                self.add_admin_proposal(
                    ProposalPayload::Upgrade(upgrade.message.clone()),
                    self.pubkey,
                    upgrade.signature_with_recovery(),
                )?;
                self.check_pending_proposals()?;
            }
            AdminMessage::Approve {
                admin_proposal_id,
                signature,
            } => {
                self.framework_state
                    .validator_set
                    .as_ref()
                    .ensure_is_validator(self.pubkey)?;

                let state = self.framework_state.admin_proposal_state.as_mut();
                let pending = state
                    .proposals
                    .get_mut(admin_proposal_id)
                    .with_context(|| {
                        format!("Specified an unknown proposal ID {admin_proposal_id}")
                    })?;

                let pubkey = signature.validate(pending.payload.as_bytes())?;
                anyhow::ensure!(pubkey == self.pubkey);

                let old_value = pending.approvals.insert(pubkey, *signature);
                anyhow::ensure!(
                    old_value.is_none(),
                    "{} already approved proposal {admin_proposal_id}",
                    self.pubkey
                );
                self.check_pending_proposals()?;
            }
        }
        Ok(())
    }

    fn check_pending_proposals(&mut self) -> Result<()> {
        if let Some((id, PendingProposal { payload, approvals })) = self.find_approved_proposal() {
            self.log_event(LogEvent::AdminProposalApproved(id))?;
            match payload {
                ProposalPayload::NewSet(validator_set) => {
                    *self.framework_state.validator_set.as_mut() = validator_set.as_inner().clone();
                    self.add_action_all_chains(ExecAction::NewSet {
                        validator_set,
                        approvals: approvals.values().copied().collect(),
                    })?;
                }
                ProposalPayload::MigrateContract(migrate_contract) => {
                    self.add_action(
                        migrate_contract.as_inner().chain(),
                        ExecAction::MigrateContract { migrate_contract },
                    )?;
                }
                ProposalPayload::Upgrade(upgrade) => {
                    self.framework_state.version = upgrade.into_inner().desired_version;
                }
            }

            // We always clear all pending proposals when an admin action completes.
            self.framework_state
                .admin_proposal_state
                .as_mut()
                .proposals
                .clear();
        }
        Ok(())
    }

    fn find_approved_proposal(&self) -> Option<(AdminProposalId, PendingProposal)> {
        for (id, pending) in &self.framework_state.admin_proposal_state.as_ref().proposals {
            if pending.has_sufficient_approvals(self.framework_state.validator_set.as_ref()) {
                return Some((*id, pending.clone()));
            }
        }

        None
    }

    pub fn log(&mut self, msg: impl Into<String>) {
        self.logs.last_mut().unwrap().push(msg.into());
    }

    pub fn log_event(&mut self, event: LogEvent) -> Result<()> {
        self.log_json(&event)
    }

    /// Log any serializable value as JSON.
    pub fn log_json<T: serde::Serialize>(&mut self, msg: &T) -> Result<()> {
        let json = serde_json::to_string(msg)?;
        self.log(json);
        Ok(())
    }
}
