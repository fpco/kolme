use std::collections::HashMap;

use cosmos::{Cosmos, HasAddressHrp, SeedPhrase, TxBuilder};

use crate::*;

/// Component which submits necessary transactions to the blockchain.
pub struct Submitter<App: KolmeApp> {
    kolme: Kolme<App>,
    seed_phrase: SeedPhrase,
    cosmos: HashMap<ExternalChain, Cosmos>,
}

impl<App: KolmeApp> Submitter<App> {
    pub fn new(kolme: Kolme<App>, seed_phrase: SeedPhrase) -> Self {
        Submitter {
            kolme,
            seed_phrase,
            cosmos: HashMap::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut receiver = self.kolme.subscribe();
        self.submit_zero_or_one().await?;
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
            self.submit_zero_or_one().await?;
        }
    }

    /// Submit 0 transactions (if nothing is needed) or the next event's transactions.
    ///
    /// We only do 0 or 1, since we always wait for listeners to confirm that our actions succeeded before continuing.
    async fn submit_zero_or_one(&mut self) -> Result<()> {
        if let Some(genesis_action) = self.kolme.read().await.get_next_genesis_action() {
            return self.handle_genesis(genesis_action).await;
        }

        // FIXME find the next action to run
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
                let cosmos = self.get_cosmos(chain).await?;
                let wallet = self.seed_phrase.with_hrp(cosmos.get_address_hrp())?;

                // TODO create a shared crate and use the same definitions in the contracts and this code
                #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
                struct InstantiateMsg {
                    processor: String,
                    executors: Vec<String>,
                    needed_executors: u16,
                }
                let msg = InstantiateMsg {
                    processor: hex::encode(processor.to_sec1_bytes()),
                    executors: executors
                        .into_iter()
                        .map(|e| hex::encode(e.to_sec1_bytes()))
                        .collect(),
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

    async fn get_cosmos(&mut self, chain: ExternalChain) -> Result<Cosmos> {
        if let Some(cosmos) = self.cosmos.get(&chain) {
            return Ok(cosmos.clone());
        }
        let cosmos = chain.make_cosmos().await?;
        self.cosmos.insert(chain, cosmos.clone());
        Ok(cosmos)
    }
}
