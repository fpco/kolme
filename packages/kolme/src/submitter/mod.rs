mod cosmos;
mod solana;

use crate::*;
use std::collections::{HashMap, HashSet};

/// Component which submits necessary transactions to the blockchain.
pub struct Submitter<App: KolmeApp> {
    kolme: Kolme<App>,
    args: ChainArgs,
    /// Keep track of which genesis contracts we've already created.
    ///
    /// Without this, we almost always end up double-instantiating the first contract.
    /// Reason: we immediately instantiate a contract, then see the new block
    /// for the genesis transaction, and then try to instantiate it again
    /// because our new contract will only be recognized in a later transaction.
    ///
    /// Simple solution: only instantiate once per chain.
    genesis_created: HashSet<ExternalChain>,
    last_submitted: HashMap<ExternalChain, BridgeActionId>,
}

enum ChainArgs {
    Cosmos {
        seed_phrase: ::cosmos::SeedPhrase,
    },
    Solana {
        keypair: kolme_solana_bridge_client::keypair::Keypair,
    },
    #[cfg(feature = "pass_through")]
    PassThrough {
        port: u16,
    },
}

impl ChainArgs {
    #[inline]
    fn can_handle(&self, chain: ExternalChain) -> bool {
        match self {
            Self::Cosmos { .. } => chain.to_cosmos_chain().is_some(),
            Self::Solana { .. } => chain.to_solana_chain().is_some(),
            #[cfg(feature = "pass_through")]
            Self::PassThrough { .. } => chain == ExternalChain::PassThrough,
        }
    }
}

impl<App: KolmeApp> Submitter<App> {
    pub fn new_cosmos(kolme: Kolme<App>, seed_phrase: ::cosmos::SeedPhrase) -> Self {
        Submitter {
            kolme,
            args: ChainArgs::Cosmos { seed_phrase },
            last_submitted: HashMap::new(),
            genesis_created: HashSet::new(),
        }
    }

    pub fn new_solana(
        kolme: Kolme<App>,
        keypair: kolme_solana_bridge_client::keypair::Keypair,
    ) -> Self {
        Submitter {
            kolme,
            args: ChainArgs::Solana { keypair },
            last_submitted: HashMap::new(),
            genesis_created: HashSet::new(),
        }
    }

    #[cfg(feature = "pass_through")]
    pub fn new_pass_through(kolme: Kolme<App>, port: u16) -> Self {
        Submitter {
            kolme,
            args: ChainArgs::PassThrough { port },
            last_submitted: HashMap::new(),
            genesis_created: HashSet::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let chains = self
            .kolme
            .read()
            .get_bridge_contracts()
            .keys()
            .filter(|x| self.args.can_handle(*x))
            .collect::<Vec<_>>();

        if chains.is_empty() {
            tracing::info!("Submitter does not support any of the configured chains. Exiting...");

            return Ok(());
        }

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
                Notification::FailedTransaction { .. } => continue,
                Notification::LatestBlock(_) => continue,
                Notification::EvictMempoolTransaction(_) => continue,
            }
            self.submit_zero_or_one(&chains).await?;
        }
    }

    /// Submit 0 transactions (if nothing is needed) or the next event's transactions.
    ///
    /// We only do 0 or 1, since we always wait for listeners to confirm that our actions succeeded before continuing.
    async fn submit_zero_or_one(&mut self, chains: &[ExternalChain]) -> Result<()> {
        // TODO we can probably unify genesis and other actions into a single per-chain feed
        let genesis_action = self.kolme.read().get_next_genesis_action();
        if let Some(genesis_action) = genesis_action {
            return self.handle_genesis(genesis_action).await;
        }

        for chain in chains {
            if let Some((action_id, action)) = self.kolme.read().get_next_bridge_action(*chain)? {
                return self.handle_bridge_action(action_id, *chain, action).await;
            }
        }

        Ok(())
    }

    async fn handle_genesis(&mut self, genesis_action: GenesisAction) -> Result<()> {
        let (contract_addr, chain) = match genesis_action {
            GenesisAction::InstantiateCosmos {
                chain,
                code_id,
                validator_set: args,
            } => {
                let ChainArgs::Cosmos { seed_phrase } = &self.args else {
                    return Ok(());
                };

                if self.genesis_created.contains(&chain.into()) {
                    return Ok(());
                }

                let cosmos = self.kolme.read().get_cosmos(chain).await?;

                let addr = cosmos::instantiate(&cosmos, seed_phrase, code_id, args).await?;

                (addr, chain.into())
            }
            GenesisAction::InstantiateSolana {
                chain,
                program_id,
                validator_set: args,
            } => {
                let ChainArgs::Solana { keypair } = &self.args else {
                    return Ok(());
                };

                if self.genesis_created.contains(&chain.into()) {
                    return Ok(());
                }

                let client = self.kolme.read().get_solana_client(chain).await;

                solana::instantiate(&client, keypair, &program_id, args).await?;

                (program_id, chain.into())
            }
        };

        self.kolme.notify(Notification::GenesisInstantiation {
            chain,
            contract: contract_addr,
        });
        self.genesis_created.insert(chain);

        Ok(())
    }

    async fn handle_bridge_action(
        &mut self,
        action_id: BridgeActionId,
        chain: ExternalChain,
        PendingBridgeAction {
            payload,
            approvals,
            processor,
        }: &PendingBridgeAction,
    ) -> Result<()> {
        let Some(processor) = processor else {
            return Ok(());
        };

        if let Some(last) = self.last_submitted.get(&chain) {
            if *last >= action_id {
                tracing::info!("Skipping submitting action {action_id} on chain {chain} - already submitted. Next expected action id: {}", last.next());

                return Ok(());
            }
        }

        let contract = {
            let kolme = self.kolme.read();
            let state = kolme.get_bridge_contracts().get(chain)?;
            match &state.config.bridge {
                BridgeContract::NeededCosmosBridge { .. }
                | BridgeContract::NeededSolanaBridge { .. } => return Ok(()),
                BridgeContract::Deployed(contract) => contract.clone(),
            }
        };

        tracing::info!("Handling bridge action {action_id} for chain: {chain:?}.");

        let tx_hash = match &self.args {
            ChainArgs::Cosmos { seed_phrase } => {
                let Some(cosmos_chain) = chain.to_cosmos_chain() else {
                    return Ok(());
                };

                let cosmos = self.kolme.read().get_cosmos(cosmos_chain).await?;

                cosmos::execute(
                    &cosmos,
                    seed_phrase,
                    &contract,
                    *processor,
                    approvals,
                    payload,
                )
                .await?
            }
            ChainArgs::Solana { keypair } => {
                let Some(solana_chain) = chain.to_solana_chain() else {
                    return Ok(());
                };

                let client = self.kolme.read().get_solana_client(solana_chain).await;

                solana::execute(
                    &client,
                    keypair,
                    &contract,
                    *processor,
                    approvals,
                    payload.clone(),
                )
                .await?
            }
            #[cfg(feature = "pass_through")]
            ChainArgs::PassThrough { port } => {
                anyhow::ensure!(chain == ExternalChain::PassThrough);
                let client = self.kolme.read().get_pass_through_client();
                pass_through::execute(client, *port, *processor, approvals, payload).await?
            }
        };

        tracing::info!(
            "Transaction submitted for {chain:?}#{action_id}: {}",
            tx_hash
        );
        self.last_submitted.insert(chain, action_id);

        Ok(())
    }
}
