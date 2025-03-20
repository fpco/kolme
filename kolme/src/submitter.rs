use std::collections::HashMap;

use cosmos::{HasAddressHrp, SeedPhrase, TxBuilder};

use crate::*;

/// Component which submits necessary transactions to the blockchain.
pub struct Submitter<App: KolmeApp> {
    kolme: Kolme<App>,
    seed_phrase: SeedPhrase,
    last_submitted: HashMap<ExternalChain, BridgeActionId>,
}

impl<App: KolmeApp> Submitter<App> {
    pub fn new(kolme: Kolme<App>, seed_phrase: SeedPhrase) -> Self {
        Submitter {
            kolme,
            seed_phrase,
            last_submitted: HashMap::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        // FIXME Looks like there may be a bug where a contract is instantiated twice on a chain, needs investigation
        // Best guess: minor race condition between the NewBlock event arriving and the state being updated, leading to basing the operation on the previous out-of-date state.

        let chains = self
            .kolme
            .read()
            .await
            .get_bridge_contracts()
            .keys()
            .copied()
            .collect::<Vec<_>>();

        let mut receiver = self.kolme.subscribe();
        self.submit_zero_or_one(&chains).await?;
        tracing::info!("Submitter has caught up, waiting for new events.");

        loop {
            match receiver.recv().await? {
                Notification::NewBlock(_) => (),
                Notification::GenesisInstantiation {
                    chain: _,
                    contract: _,
                } => continue,
                Notification::Broadcast { tx: _ } => continue,
            }
            self.submit_zero_or_one(&chains).await?;
        }
    }

    /// Submit 0 transactions (if nothing is needed) or the next event's transactions.
    ///
    /// We only do 0 or 1, since we always wait for listeners to confirm that our actions succeeded before continuing.
    async fn submit_zero_or_one(&mut self, chains: &[ExternalChain]) -> Result<()> {
        // TODO we can probably unify genesis and other actions into a single per-chain feed
        let genesis_action = self.kolme.read().await.get_next_genesis_action();
        if let Some(genesis_action) = genesis_action {
            return self.handle_genesis(genesis_action).await;
        }

        for chain in chains {
            if let Some(bridge_action) = self
                .kolme
                .read()
                .await
                .get_next_bridge_action(*chain)
                .await?
            {
                return self.handle_bridge_action(bridge_action).await;
            }
        }

        Ok(())
    }

    async fn handle_genesis(&mut self, genesis_action: GenesisAction) -> Result<()> {
        match genesis_action {
            GenesisAction::InstantiateCosmos {
                chain,
                code_id,
                processor,
                listeners: _,
                needed_listeners: _,
                executors,
                needed_executors,
            } => {
                let cosmos = self.kolme.read().await.get_cosmos(chain).await?;
                let wallet = self.seed_phrase.with_hrp(cosmos.get_address_hrp())?;

                // TODO create a shared crate and use the same definitions in the contracts and this code
                #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
                struct InstantiateMsg {
                    processor: PublicKey,
                    executors: Vec<PublicKey>,
                    needed_executors: u16,
                }
                let msg = InstantiateMsg {
                    processor,
                    executors: executors.into_iter().collect(),
                    needed_executors: needed_executors.try_into()?,
                };

                let contract = cosmos
                    .make_code_id(code_id)
                    .instantiate(
                        &wallet,
                        "Kolme Framework Bridge Contract".to_owned(),
                        vec![],
                        &msg,
                        cosmos::ContractAdmin::Sender,
                    )
                    .await?;
                tracing::info!("Instantiate new contract: {contract}");
                let res = TxBuilder::default()
                    .add_update_contract_admin(&contract, &wallet, &contract)
                    .sign_and_broadcast(&cosmos, &wallet)
                    .await?;
                tracing::info!(
                    "Updated admin on {contract} to its own address in tx {}",
                    res.txhash
                );
                self.kolme
                    .notify_genesis_instantiation(chain, contract.to_string());
                Ok(())
            }
        }
    }

    async fn handle_bridge_action(
        &mut self,
        PendingBridgeAction {
            chain,
            payload,
            height,
            message: msg_index,
            action_id,
        }: PendingBridgeAction,
    ) -> Result<()> {
        if let Some(last) = self.last_submitted.get(&chain) {
            if *last <= action_id {
                return Ok(());
            }
        }

        let block = self.kolme.read().await.load_block(height).await?;
        let message = block
            .0
            .message
            .as_inner()
            .tx
            .0
            .message
            .as_inner()
            .messages
            .get(msg_index)
            .with_context(|| format!("Block height {height} is missing message #{msg_index}"))?;

        let Message::ProcessorApprove {
            chain: chain2,
            action_id: action_id2,
            processor,
            executors,
        } = message
        else {
            anyhow::bail!("Wrong message type for {height}#{msg_index}");
        };
        anyhow::ensure!(&chain == chain2);
        anyhow::ensure!(&action_id == action_id2);

        let msg = ExecuteMsg::Signed {
            processor,
            executors,
            payload,
        };
        let contract = {
            let kolme = self.kolme.read().await;
            let cosmos = kolme.get_cosmos(chain).await?;
            match kolme.get_bridge_contracts().get(&chain) {
                None => return Ok(()),
                Some(config) => match &config.bridge {
                    BridgeContract::NeededCosmosBridge { code_id: _ } => return Ok(()),
                    BridgeContract::Deployed(contract) => cosmos.make_contract(contract.parse()?),
                },
            }
        };
        let tx = contract
            .execute(
                &self.seed_phrase.with_hrp(contract.get_address_hrp())?,
                vec![],
                msg,
            )
            .await?;
        tracing::info!(
            "Transaction submitted for {chain:?}#{action_id}: {}",
            tx.txhash
        );
        self.last_submitted.insert(chain, action_id);
        Ok(())
    }
}
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg<'a> {
    Signed {
        processor: &'a SignatureWithRecovery,
        executors: &'a [SignatureWithRecovery],
        payload: String,
    },
}
