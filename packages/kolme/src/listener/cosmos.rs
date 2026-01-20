use std::mem;

use super::get_next_bridge_event_id;
use crate::*;
use ::cosmos::{Contract, Cosmos};
use cosmos::error::AddressError;
use cosmwasm_std::Coin;
use shared::cosmos::{BridgeEventMessage, GetEventResp, QueryMsg};

#[derive(thiserror::Error, Debug)]
pub enum CosmosListenerError {
    #[error("Code ID mismatch: expected {expected}, actual {actual}")]
    CodeId { expected: u64, actual: u64 },

    #[error("Processor mismatch")]
    Processor,

    #[error("Listeners mismatch")]
    Listeners,

    #[error("Needed listeners mismatch")]
    NeededListeners,

    #[error("Approvers mismatch")]
    Approvers,

    #[error("Needed approvers mismatch")]
    NeededApprovers,

    #[error("Invalid contract address: {0}")]
    InvalidAddress(#[from] AddressError),

    #[error(transparent)]
    CosmosError(#[from] cosmos::Error),
}

pub async fn listen<App: KolmeApp>(
    kolme: Kolme<App>,
    secret: SecretKey,
    chain: CosmosChain,
    contract: String,
) -> Result<(), KolmeError> {
    let kolme_r = kolme.read();

    let cosmos = kolme_r.get_cosmos(chain).await?;
    let contract = cosmos.make_contract(contract.parse()?);

    let mut next_bridge_event_id =
        get_next_bridge_event_id(&kolme_r, secret.public_key(), chain.into());

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
) -> Result<(), KolmeError> {
    match contract
        .query(&QueryMsg::GetEvent {
            id: *next_bridge_event_id,
        })
        .await
        .map_err(KolmeError::from)?
    {
        GetEventResp::Found { message } => {
            let message =
                serde_json::from_slice::<BridgeEventMessage>(&message).map_err(KolmeError::from)?;
            let message =
                to_kolme_message::<App::Message>(message, chain.into(), *next_bridge_event_id);

            kolme
                .sign_propose_await_transaction(secret, vec![message])
                .await?;

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
) -> Result<(), CosmosListenerError> {
    let contract = cosmos.make_contract(
        contract
            .parse::<cosmos::Address>()
            .map_err(CosmosListenerError::InvalidAddress)?,
    );
    let actual_code_id = contract
        .info()
        .await
        .map_err(CosmosListenerError::from)?
        .code_id;

    if actual_code_id != expected_code_id {
        return Err(CosmosListenerError::CodeId {
            expected: expected_code_id,
            actual: actual_code_id,
        });
    }

    let shared::cosmos::State {
        set:
            ValidatorSet {
                processor,
                approvers,
                needed_approvers,
                listeners,
                needed_listeners,
            },
        next_event_id: _,
        next_action_id: _,
    } = contract
        .query(shared::cosmos::QueryMsg::Config {})
        .await
        .map_err(CosmosListenerError::from)?;

    if info.validator_set.processor != processor {
        return Err(CosmosListenerError::Processor);
    }
    if listeners != info.validator_set.listeners {
        return Err(CosmosListenerError::Listeners);
    }

    if needed_listeners != info.validator_set.needed_listeners {
        return Err(CosmosListenerError::NeededListeners);
    }

    if approvers != info.validator_set.approvers {
        return Err(CosmosListenerError::Approvers);
    }

    if needed_approvers != info.validator_set.needed_approvers {
        return Err(CosmosListenerError::NeededApprovers);
    }

    Ok(())
}

fn to_kolme_message<T>(
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

            for Coin { denom, amount } in funds {
                let amount = amount.u128();
                new_funds.push(BridgedAssetAmount { denom, amount });
            }

            Message::Listener {
                chain,
                event_id,
                event: BridgeEvent::Regular {
                    wallet: Wallet(wallet),
                    funds: new_funds,
                    keys,
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
