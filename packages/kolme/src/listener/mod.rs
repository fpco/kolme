#[cfg(feature = "cosmwasm")]
mod cosmos;
#[cfg(feature = "solana")]
mod solana;

use crate::*;
use tokio::task::JoinSet;

pub struct Listener<App: KolmeApp> {
    kolme: Kolme<App>,
    secret: SecretKey,
}

pub(crate) fn get_next_bridge_event_id<App: KolmeApp>(
    kolme: &KolmeRead<App>,
    public: PublicKey,
    chain: ExternalChain,
) -> BridgeEventId {
    let state = kolme.get_bridge_contracts().get(chain).unwrap();

    for (event_id, pending) in &state.pending_events {
        if !pending.attestations.contains(&public) {
            return *event_id;
        }
    }

    state.next_event_id
}

impl<App: KolmeApp> Listener<App> {
    pub fn new(kolme: Kolme<App>, secret: SecretKey) -> Self {
        Listener { kolme, secret }
    }

    pub async fn run(self, name: ChainName) -> Result<()> {
        let mut set = JoinSet::new();
        tracing::debug!("Listen on {name:?}");

        match name {
            ChainName::Cosmos => {
                #[cfg(feature = "cosmwasm")]
                {
                    let contracts = self.wait_for_contracts(name).await?;
                    for (chain, contract) in contracts {
                        set.spawn(cosmos::listen(
                            self.kolme.clone(),
                            self.secret.clone(),
                            chain.to_cosmos_chain().unwrap(),
                            contract,
                        ));
                    }
                }
            }
            ChainName::Solana => {
                #[cfg(feature = "solana")]
                {
                    let contracts = self.wait_for_contracts(name).await?;
                    for (chain, contract) in contracts {
                        set.spawn(absurd_future::absurd_future(solana::listen(
                            self.kolme.clone(),
                            self.secret.clone(),
                            chain.to_solana_chain().unwrap(),
                            contract,
                        )));
                    }
                }
            }
            ChainName::Ethereum => {
                tracing::warn!(
                    "Ethereum listener requested, but Ethereum listener support is not implemented yet."
                );
            }
            #[cfg(feature = "pass_through")]
            ChainName::PassThrough => {
                let contracts = self.wait_for_contracts(name).await?;
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
        let mut new_block = self.kolme.subscribe_new_block();
        let mut mempool = self.kolme.subscribe_mempool_additions();
        loop {
            // Did the contracts land on-chain?
            if let Some(contracts) = self.get_contracts(name) {
                return Ok(contracts);
            }

            // Wait for any changes in the mempool or new blocks
            tokio::select! {
                _ = new_block.listen() => (),
                _ = mempool.listen() => (),
            }

            // Then look for any contract instantiation messages.
            for msg in self
                .kolme
                .get_mempool_entries()
                .iter()
                .flat_map(|tx| tx.0.message.as_inner().messages.iter())
            {
                if let Message::Listener {
                    chain,
                    event_id,
                    event: BridgeEvent::Instantiated { contract },
                } = msg
                {
                    if chain.name() != name || *event_id != BridgeEventId::start() {
                        continue;
                    }

                    self.try_new_contract(*chain, contract).await?;
                }
            }
        }
    }

    async fn try_new_contract(&self, chain: ExternalChain, contract: &str) -> Result<()> {
        let kolme = self.kolme.read();
        let next = get_next_bridge_event_id(&kolme, self.secret.public_key(), chain);
        if next != BridgeEventId::start() {
            return Ok(());
        }
        let config = &kolme.get_bridge_contracts().get(chain)?.config;

        if let BridgeContract::Deployed(_) = config.bridge {
            anyhow::bail!("Already have a deployed contract on {chain:?}")
        };

        let res: Result<()> = match ChainKind::from(chain) {
            #[cfg(feature = "cosmwasm")]
            ChainKind::Cosmos(chain) => {
                let cosmos = kolme.get_cosmos(chain).await?;
                let expected_code_id = match config.bridge {
                    BridgeContract::NeededCosmosBridge { code_id } => code_id,
                    BridgeContract::NeededSolanaBridge { .. } => unreachable!(),
                    BridgeContract::Deployed(_) => {
                        anyhow::bail!("Already have a deployed contract on {chain:?}")
                    }
                };

                cosmos::sanity_check_contract(
                    &cosmos,
                    contract,
                    expected_code_id,
                    self.kolme.get_app().genesis_info(),
                )
                .await
            }
            #[cfg(not(feature = "cosmwasm"))]
            ChainKind::Cosmos(_) => Ok(()),
            #[cfg(feature = "solana")]
            ChainKind::Solana(chain) => {
                let client = kolme.get_solana_client(chain).await;

                solana::sanity_check_contract(
                    &client,
                    contract,
                    self.kolme.get_app().genesis_info(),
                )
                .await
            }
            #[cfg(not(feature = "solana"))]
            ChainKind::Solana(_) => Ok(()),
            ChainKind::Ethereum(_) => {
                anyhow::bail!("Ethereum listener contract checks are not implemented yet")
            }
            #[cfg(feature = "pass_through")]
            ChainKind::PassThrough => {
                anyhow::bail!("No wait for pass-through contract is expected")
            }
        };

        if let Err(e) = res {
            tracing::error!("Invalid genesis contract {contract} on {chain:?} found: {e}");
        }

        kolme
            .sign_propose_await_transaction(
                &self.secret,
                vec![Message::Listener {
                    chain,
                    event: BridgeEvent::Instantiated {
                        contract: contract.to_owned(),
                    },
                    event_id: BridgeEventId::start(),
                }],
            )
            .await?;
        Ok(())
    }

    fn get_contracts(&self, name: ChainName) -> Option<BTreeMap<ExternalChain, String>> {
        let mut res = BTreeMap::new();

        for (chain, state) in self.kolme.read().get_bridge_contracts().iter() {
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
