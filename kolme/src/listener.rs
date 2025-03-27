use cosmos::Contract;
use cosmwasm_std::Coin;
use shared::cosmos::{BridgeEventMessage, GetEventResp, QueryMsg};
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

    pub async fn run(self) -> Result<()> {
        let contracts = self.wait_for_contracts().await?;
        let mut set = JoinSet::new();
        for (chain, contract) in contracts {
            set.spawn(listen(
                self.kolme.clone(),
                self.secret.clone(),
                chain,
                contract,
            ));
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

    async fn wait_for_contracts(&self) -> Result<BTreeMap<ExternalChain, String>> {
        let mut receiver = self.kolme.subscribe();
        loop {
            if let Some(contracts) = self.get_contracts().await {
                return Ok(contracts);
            }
            if let Notification::GenesisInstantiation { chain, contract } = receiver.recv().await? {
                let kolme = self.kolme.read().await;
                if !kolme
                    .received_listener_attestation(
                        chain,
                        self.secret.public_key(),
                        BridgeEventId::start(),
                    )
                    .await?
                {
                    if let Err(e) = self.sanity_check_contract(chain, &contract).await {
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

    async fn get_contracts(&self) -> Option<BTreeMap<ExternalChain, String>> {
        let mut res = BTreeMap::new();
        for (chain, config) in self.kolme.read().await.get_bridge_contracts() {
            match &config.bridge {
                BridgeContract::NeededCosmosBridge { code_id: _ } => return None,
                BridgeContract::Deployed(contract) => {
                    res.insert(*chain, contract.clone());
                }
            }
        }
        Some(res)
    }

    async fn sanity_check_contract(&self, chain: ExternalChain, contract: &str) -> Result<()> {
        let kolme = self.kolme.read().await;
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
        let contract = kolme
            .get_cosmos(chain)
            .await?
            .make_contract(contract.parse()?);
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

        let info = App::genesis_info();

        anyhow::ensure!(info.processor == processor);

        anyhow::ensure!(approvers == info.approvers);
        anyhow::ensure!(usize::from(needed_approvers) == info.needed_approvers);

        Ok(())
    }
}

async fn listen<App: KolmeApp>(
    kolme: Kolme<App>,
    secret: SecretKey,
    chain: ExternalChain,
    contract: String,
) -> Result<()> {
    let cosmos = kolme.read().await.get_cosmos(chain).await?;
    let contract = cosmos.make_contract(contract.parse()?);
    let mut next_bridge_event_id = kolme
        .read()
        .await
        .get_next_bridge_event_id(chain, secret.public_key())
        .await?;
    tracing::info!(
        "Beginning listener loop on contract {contract}, next event ID: {next_bridge_event_id}"
    );
    // We _should_ be subscribing to events. I tried doing that and failed miserably.
    // So we're trying this polling approach instead.
    loop {
        listen_once(&kolme, &secret, chain, &contract, &mut next_bridge_event_id).await?;
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
    }
}

async fn listen_once<App: KolmeApp>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
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
            broadcast_listener_event(kolme, secret, chain, *next_bridge_event_id, &message).await?;
            *next_bridge_event_id = next_bridge_event_id.next();
            Ok(())
        }
        GetEventResp::NotFound {} => Ok(()),
    }
}

async fn broadcast_listener_event<App: KolmeApp>(
    kolme: &Kolme<App>,
    secret: &SecretKey,
    chain: ExternalChain,
    bridge_event_id: BridgeEventId,
    message: &BridgeEventMessage,
) -> Result<()> {
    let message = match message {
        BridgeEventMessage::Regular {
            wallet,
            funds,
            keys,
        } => {
            let mut new_funds = vec![];
            let mut new_keys = vec![];

            for Coin { denom, amount } in funds {
                let amount = amount.u128();
                new_funds.push(BridgedAssetAmount {
                    denom: denom.clone(),
                    amount,
                });
            }
            for key in keys {
                new_keys.push(*key);
            }

            Message::Listener {
                chain,
                event_id: bridge_event_id,
                event: BridgeEvent::Regular {
                    wallet: wallet.clone(),
                    funds: new_funds,
                    keys: new_keys,
                },
            }
        }
        BridgeEventMessage::Signed { wallet, action_id } => Message::Listener {
            chain,
            event_id: bridge_event_id,
            event: BridgeEvent::Signed {
                wallet: wallet.clone(),
                action_id: *action_id,
            },
        },
    };
    let signed = kolme
        .read()
        .await
        .create_signed_transaction(secret, vec![message])
        .await?;
    kolme.propose_transaction(signed)?;
    Ok(())
}
