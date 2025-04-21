use std::mem;

use crate::*;
use ::cosmos::{Contract, Cosmos};
use cosmwasm_std::Coin;
use shared::cosmos::{BridgeEventMessage, GetEventResp, QueryMsg};

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
                to_kolme_message::<App::Message>(message, chain.into(), *next_bridge_event_id);

            let signed = kolme
                .read()
                .await
                .create_signed_transaction(secret, vec![message])
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

pub(crate) fn to_kolme_message<T>(
    msg: BridgeEventMessage,
    chain: ExternalChain,
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
                chain,
                event_id,
                event: BridgeEvent::Regular {
                    wallet: Wallet(wallet),
                    funds: new_funds,
                    keys: new_keys,
                },
            }
        }
        BridgeEventMessage::Signed { wallet, action_id } => Message::Listener {
            chain,
            event_id,
            event: BridgeEvent::Signed {
                wallet: Wallet(wallet),
                action_id,
            },
        },
    }
}
