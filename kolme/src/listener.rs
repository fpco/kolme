use cosmos::Contract;
use cosmwasm_std::Coin;
use shared::cosmos::{BridgeEventMessage, GetEventResp, QueryMsg};
use std::mem;
use tokio::task::JoinSet;

use crate::*;

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

        match name {
            ChainName::Cosmos => {
                for (chain, contract) in contracts {
                    set.spawn(cosmos_listener::listen(
                        self.kolme.clone(),
                        self.secret.clone(),
                        chain.to_cosmos_chain().unwrap(),
                        contract,
                    ));
                }
            }
            ChainName::Solana => {
                for (chain, contract) in contracts {
                    set.spawn(solana_listener::listen(
                        self.kolme.clone(),
                        self.secret.clone(),
                        chain.to_solana_chain().unwrap(),
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
                if !kolme
                    .received_listener_attestation(
                        chain,
                        self.secret.public_key(),
                        BridgeEventId::start(),
                    )
                    .await?
                {
                    let config = kolme
                        .get_bridge_contracts()
                        .get(&chain)
                        .with_context(|| format!("No chain config found for {chain:?}"))?;

                    let expected_code_id = match config.bridge {
                        BridgeContract::NeededCosmosBridge { code_id } => code_id,
                        BridgeContract::Deployed(_) => {
                            anyhow::bail!("Already have a deployed contract on {chain:?}")
                        }
                    };

                    let res = match ChainKind::from(chain) {
                        ChainKind::Cosmos(chain) => {
                            let cosmos = kolme.get_cosmos(chain).await?;

                            cosmos_listener::sanity_check_contract(
                                &cosmos,
                                &contract,
                                expected_code_id,
                                &App::genesis_info(),
                            )
                            .await
                        }
                        ChainKind::Solana(chain) => {
                            let client = kolme.get_solana_client(chain).await;

                            solana_listener::sanity_check_contract(
                                &client,
                                &contract,
                                &App::genesis_info(),
                            )
                            .await
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

        for (chain, config) in self.kolme.read().await.get_bridge_contracts() {
            if chain.name() != name {
                continue;
            }

            match &config.bridge {
                BridgeContract::NeededCosmosBridge { code_id: _ } => return None,
                BridgeContract::Deployed(contract) => {
                    res.insert(*chain, contract.clone());
                }
            }
        }

        Some(res)
    }
}

mod cosmos_listener {
    use super::*;
    use cosmos::Cosmos;

    pub async fn listen<App: KolmeApp>(
        kolme: Kolme<App>,
        secret: SecretKey,
        chain: CosmosChain,
        contract: String,
    ) -> Result<()> {
        let kolme_r = kolme.read().await;

        let cosmos = kolme_r.get_cosmos(chain).await?;
        let contract = cosmos.make_contract(contract.parse()?);

        let mut next_bridge_event_id = kolme_r
            .get_next_bridge_event_id(chain.into(), secret.public_key())
            .await?;

        mem::drop(kolme_r);

        tracing::info!(
            "Beginning listener loop on contract {contract}, next event ID: {next_bridge_event_id}"
        );

        // We _should_ be subscribing to events. I tried doing that and failed miserably.
        // So we're trying this polling approach instead.
        loop {
            listen_once(&kolme, &secret, chain, &contract, &mut next_bridge_event_id).await?;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    async fn listen_once<App: KolmeApp>(
        kolme: &Kolme<App>,
        secret: &SecretKey,
        chain: CosmosChain,
        contract: &Contract,
        next_bridge_event_id: &mut BridgeEventId,
    ) -> Result<()> {
        match contract
            .query(&QueryMsg::GetEvent {
                id: *next_bridge_event_id,
            })
            .await?
        {
            GetEventResp::Found { message } => {
                let message = serde_json::from_slice::<BridgeEventMessage>(&message)?;
                let message =
                    to_kolme_message::<App::Message>(message, chain, *next_bridge_event_id);

                let signed = kolme
                    .read()
                    .await
                    .create_signed_transaction(&secret, vec![message])
                    .await?;

                kolme.propose_transaction(signed)?;

                *next_bridge_event_id = next_bridge_event_id.next();

                Ok(())
            }
            GetEventResp::NotFound {} => Ok(()),
        }
    }

    pub async fn sanity_check_contract(
        cosmos: &Cosmos,
        contract: &str,
        expected_code_id: u64,
        info: &GenesisInfo,
    ) -> Result<()> {
        let contract = cosmos.make_contract(contract.parse()?);
        let actual_code_id = contract.info().await?.code_id;

        anyhow::ensure!(
            actual_code_id == expected_code_id,
            "Code ID mismatch, expected {expected_code_id}, but {contract} has {actual_code_id}"
        );

        let shared::cosmos::State {
            processor,
            approvers,
            needed_approvers,
            next_event_id: _,
            next_action_id: _,
        } = contract.query(shared::cosmos::QueryMsg::Config {}).await?;

        anyhow::ensure!(info.processor == processor);
        anyhow::ensure!(approvers == info.approvers);
        anyhow::ensure!(usize::from(needed_approvers) == info.needed_approvers);

        Ok(())
    }

    fn to_kolme_message<T>(
        msg: BridgeEventMessage,
        chain: CosmosChain,
        event_id: BridgeEventId,
    ) -> Message<T> {
        match msg {
            BridgeEventMessage::Regular {
                wallet,
                funds,
                keys,
            } => {
                let mut new_funds = Vec::with_capacity(funds.len());
                let mut new_keys = Vec::with_capacity(keys.len());

                for Coin { denom, amount } in funds {
                    let amount = amount.u128();
                    new_funds.push(BridgedAssetAmount { denom, amount });
                }

                for key in keys {
                    new_keys.push(key);
                }

                Message::Listener {
                    chain: chain.into(),
                    event_id,
                    event: BridgeEvent::Regular {
                        wallet,
                        funds: new_funds,
                        keys: new_keys,
                    },
                }
            }
            BridgeEventMessage::Signed { wallet, action_id } => Message::Listener {
                chain: chain.into(),
                event_id,
                event: BridgeEvent::Signed { wallet, action_id },
            },
        }
    }
}

mod solana_listener {
    use std::{ops::Deref, str::FromStr};

    use base64::Engine;
    use borsh::de::BorshDeserialize;
    use kolme_solana_bridge_client::{
        solana_pubkey::Pubkey, BridgeMessage, Message as ContractMessage, State as BridgeState,
    };
    use libp2p::futures::StreamExt;
    use solana_client::rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter};

    use super::*;

    pub async fn listen<App: KolmeApp>(
        kolme: Kolme<App>,
        secret: SecretKey,
        chain: SolanaChain,
        contract: String,
    ) -> Result<()> {
        const PROGRAM_DATA_LOG: &str = "Program data: ";

        let client = chain.make_pubsub_client().await?;
        let mut next_bridge_event_id = kolme
            .read()
            .await
            .get_next_bridge_event_id(chain.into(), secret.public_key())
            .await?;

        let filter = RpcTransactionLogsFilter::Mentions(vec![contract.clone()]);
        let config = RpcTransactionLogsConfig {
            commitment: None, // Defaults to finalized
        };

        let (mut subscription, unsub) = client.logs_subscribe(filter, config).await?;

        tracing::info!(
            "Beginning listener loop on contract {contract}, next event ID: {next_bridge_event_id}"
        );

        // TODO: How do we handle disconnects and missed updates?
        'subscription_loop: while let Some(resp) = subscription.next().await {
            // We don't care about unsuccessful transactions.
            if resp.value.err.is_some() {
                continue;
            }

            let mut msg: Option<BridgeMessage> = None;

            // Our program data should always be the last "Program data:" entry even if CPI was invoked.
            for log in resp.value.logs.into_iter().rev() {
                if !log.starts_with(PROGRAM_DATA_LOG) {
                    continue;
                }

                let data = &log.as_str()[PROGRAM_DATA_LOG.len()..];
                let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;

                let result =
                    <BridgeMessage as BorshDeserialize>::try_from_slice(&bytes).map_err(|x| {
                        anyhow::anyhow!(
                            "Error deserializing Solana bridge message from logs: {:?}",
                            x
                        )
                    })?;

                if next_bridge_event_id.0 != result.id {
                    tracing::warn!(
                        "Received bridge message with ID {} but expected ID {}. Ignoring...",
                        next_bridge_event_id.0,
                        result.id
                    );

                    continue 'subscription_loop;
                }

                msg = Some(result);

                break;
            }

            if let Some(msg) = msg {
                let msg = to_kolme_message::<App::Message>(msg, chain);

                let signed = kolme
                    .read()
                    .await
                    .create_signed_transaction(&secret, vec![msg])
                    .await?;

                kolme.propose_transaction(signed)?;

                next_bridge_event_id = next_bridge_event_id.next();
            } else {
                tracing::error!("No bridge message data log was found in {contract} logs.");
            }
        }

        (unsub)().await;

        Ok(())
    }

    pub async fn sanity_check_contract(
        client: &SolanaClient,
        program: &str,
        info: &GenesisInfo,
    ) -> Result<()> {
        let program_id = Pubkey::from_str(program)?;
        let state_acc = kolme_solana_bridge_client::derive_state_pda(&program_id);

        let acc = client.get_account(&state_acc).await?;

        if acc.owner != program_id || acc.data.is_empty() {
            return Err(anyhow::anyhow!(
                "Bridge program {program} hasn't been initialized yet."
            ));
        }

        let state = <BridgeState as BorshDeserialize>::try_from_slice(&acc.data)
            .map_err(|x| anyhow::anyhow!("Error deserializing Solana bridge state: {:?}", x))?;

        anyhow::ensure!(
            info.processor.as_bytes().deref() == state.processor.to_sec1_bytes().as_slice()
        );
        anyhow::ensure!(info.approvers.len() == state.executors.len());

        for a in &state.executors {
            anyhow::ensure!(info
                .approvers
                .contains(&PublicKey::try_from_bytes(a.to_sec1_bytes().as_slice())?));
        }

        anyhow::ensure!(info.needed_approvers == usize::from(state.needed_executors));

        Ok(())
    }

    fn to_kolme_message<T>(msg: BridgeMessage, chain: SolanaChain) -> Message<T> {
        let event_id = BridgeEventId(msg.id);
        let wallet = Pubkey::new_from_array(msg.wallet).to_string();
        let event = match msg.ty {
            ContractMessage::Regular { funds, keys } => {
                let mut new_funds = Vec::with_capacity(funds.len());
                let mut new_keys = Vec::with_capacity(keys.len());

                for coin in funds {
                    new_funds.push(BridgedAssetAmount {
                        denom: Pubkey::new_from_array(coin.mint).to_string(),
                        amount: coin.amount.into(),
                    });
                }

                for key in keys {
                    if let Ok(key) = PublicKey::try_from_bytes(key.to_sec1_bytes().as_slice()) {
                        new_keys.push(key);
                    }
                }

                // TODO: Do we still need to emit if both funds and keys are empty?
                BridgeEvent::Regular {
                    wallet,
                    funds: new_funds,
                    keys: new_keys,
                }
            }
            ContractMessage::Signed { action_id } => BridgeEvent::Signed {
                wallet,
                action_id: BridgeActionId(action_id.into()),
            },
        };

        Message::Listener {
            chain: chain.into(),
            event_id,
            event,
        }
    }
}
