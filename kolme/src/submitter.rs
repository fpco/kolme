use cosmos::{HasAddressHrp, SeedPhrase, TxBuilder};

use crate::*;

/// Component which submits necessary transactions to the blockchain.
pub struct Submitter<App: KolmeApp> {
    kolme: Kolme<App>,
    seed_phrase: SeedPhrase,
}

impl<App: KolmeApp> Submitter<App> {
    pub fn new(kolme: Kolme<App>, seed_phrase: SeedPhrase) -> Self {
        Submitter { kolme, seed_phrase }
    }

    pub async fn run(mut self) -> Result<()> {
        // FIXME Looks like there may be a bug where a contract is instantiated twice on a chain, needs investigation
        // Best guess: minor race condition between the NewBlock event arriving and the state being updated, leading to basing the operation on the previous out-of-date state.

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
}
