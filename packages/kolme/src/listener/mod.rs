pub(crate) mod cosmos;
mod solana;

use crate::*;
use cosmos::get_next_bridge_event_id;
use tokio::task::JoinSet;

pub struct Listener<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
}

impl<App: KolmeApp> Listener<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Listener { kolme, secret }
    }

    pub async fn run(self, name: ChainName) -> Result<()> {
        let contracts = self.wait_for_contracts(name).await?;
        let mut set = JoinSet::new();
        tracing::debug!("Listen on {name:?}");

        match name {
            ChainName::Cosmos => {
                for (chain, contract) in contracts {
                    set.spawn(cosmos::listen(
                        self.kolme.clone(),
                        self.secret.clone(),
                        chain.to_cosmos_chain().unwrap(),
                        contract,
                    ));
                }
            }
            ChainName::Solana => {
                for (chain, contract) in contracts {
                    set.spawn(solana::listen(
                        self.kolme.clone(),
                        self.secret.clone(),
                        chain.to_solana_chain().unwrap(),
                        contract,
                    ));
                }
            }
            #[cfg(feature = "pass_through")]
            ChainName::PassThrough => {
                for (chain, contract) in contracts {
                    assert!(chain == ExternalChain::PassThrough);
                    set.spawn(pass_through::listen(
                        self.kolme.clone(),
                        self.secret.clone(),
                        contract,
                    ));
                }
            }
        }

        while let Some(res) = set.join_next().await {
            match res {
                Err(e) => {
                    set.abort_all();
                    return Err(anyhow::anyhow!("Listener panicked: {e}"));
                }
                Ok(Err(e)) => return Err(e),
                Ok(Ok(())) => (),
            }
        }

        Ok(())
    }

    async fn wait_for_contracts(&self, name: ChainName) -> Result<BTreeMap<ExternalChain, String>> {
        let mut receiver = self.kolme.subscribe();
        loop {
            if let Some(contracts) = self.get_contracts(name).await {
                return Ok(contracts);
            }

            if let Notification::GenesisInstantiation { chain, contract } = receiver.recv().await? {
                if chain.name() != name {
                    continue;
                }

                let kolme = self.kolme.read().await;
                let next = get_next_bridge_event_id(&kolme, self.secret.public_key(), chain);
                if next == BridgeEventId::start() {
                    let config = &kolme.get_bridge_contracts().get(chain)?.config;

                    let expected_code_id = match config.bridge {
                        BridgeContract::NeededCosmosBridge { code_id } => code_id,
                        BridgeContract::NeededSolanaBridge { .. } => 0, // Solana has no code id to check
                        BridgeContract::Deployed(_) => {
                            anyhow::bail!("Already have a deployed contract on {chain:?}")
                        }
                    };

                    let res = match ChainKind::from(chain) {
                        ChainKind::Cosmos(chain) => {
                            let cosmos = kolme.get_cosmos(chain).await?;

                            cosmos::sanity_check_contract(
                                &cosmos,
                                &contract,
                                expected_code_id,
                                &App::genesis_info(),
                            )
                            .await
                        }
                        ChainKind::Solana(chain) => {
                            let client = kolme.get_solana_client(chain).await;

                            solana::sanity_check_contract(&client, &contract, &App::genesis_info())
                                .await
                        }
                        #[cfg(feature = "pass_through")]
                        ChainKind::PassThrough => {
                            anyhow::bail!("No wait for pass-through contract is expected")
                        }
                    };

                    if let Err(e) = res {
                        tracing::error!(
                            "Invalid genesis contract {contract} on {chain:?} found: {e}"
                        );
                    }

                    let signed = kolme
                        .create_signed_transaction(
                            &self.secret,
                            vec![Message::Listener {
                                chain,
                                event: BridgeEvent::Instantiated { contract },
                                event_id: BridgeEventId::start(),
                            }],
                        )
                        .await?;
                    self.kolme.propose_transaction(signed)?;
                }
            }
        }
    }

    async fn get_contracts(&self, name: ChainName) -> Option<BTreeMap<ExternalChain, String>> {
        let mut res = BTreeMap::new();

        for (chain, state) in self.kolme.read().await.get_bridge_contracts().iter() {
            if chain.name() != name {
                continue;
            }

            match &state.config.bridge {
                BridgeContract::NeededCosmosBridge { .. }
                | BridgeContract::NeededSolanaBridge { .. } => return None,
                BridgeContract::Deployed(contract) => {
                    res.insert(chain, contract.clone());
                }
            }
        }

        Some(res)
    }
}
