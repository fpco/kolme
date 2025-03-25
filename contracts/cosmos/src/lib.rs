mod signing;

use std::num::TryFromIntError;

use cosmwasm_std::{
    entry_point, from_json, to_json_binary, Binary, Deps, DepsMut, Env, Event, MessageInfo,
    Response, Storage,
};
use cw_storage_plus::{Item, Map};
use sha2::{Digest, Sha256};
use shared::{
    cosmos::*,
    cryptography::PublicKey,
    types::{BridgeActionId, BridgeEventId},
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cosmwasm {
        #[from]
        source: cosmwasm_std::StdError,
    },
    #[error(transparent)]
    Signing {
        #[from]
        source: signing::SignatureError,
    },
    #[error("No executors provided")]
    NoExecutorsProvided,
    #[error("Too many executors provided")]
    TooManyExecutors(TryFromIntError),
    #[error("Need at least {needed} executors, but only {provided} provided.")]
    InsufficientExecutors { needed: u16, provided: u16 },
    #[error("Incorrect action ID. Expected: {expected}. Received: {received}.")]
    IncorrectActionId {
        expected: BridgeActionId,
        received: BridgeActionId,
    },
    #[error("Insufficient executor signatures provided. Needed: {needed}. Provided: {provided}.")]
    InsufficientSignatures { needed: u16, provided: u16 },
    #[error("Public key {key} is not part of the executor set.")]
    NonExecutorKey { key: PublicKey },
    #[error("Duplicate public key provided: {key}.")]
    DuplicateKey { key: PublicKey },
    #[error("Processor signature had the wrong public key. Expected key {expected}. Actually signed with {actual}.")]
    NonProcessorKey {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

const STATE: Item<shared::cosmos::State> = Item::new("s");

/// Outgoing events to be picked up by listeners.
///
/// This is _not_ the way we want to write things! There
/// shouldn't be on-chain storage for events, just logs.
/// However, I'm struggling to make the tx_search endpoints
/// work correctly, so I'm cheating a bit.
const EVENTS: Map<BridgeEventId, Binary> = Map::new("t");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    InstantiateMsg {
        processor,
        executors,
        needed_executors,
    }: InstantiateMsg,
) -> Result<Response> {
    if executors.is_empty() {
        return Err(Error::NoExecutorsProvided);
    }
    let executors_len = u16::try_from(executors.len()).map_err(Error::TooManyExecutors)?;
    if executors_len < needed_executors {
        return Err(Error::InsufficientExecutors {
            needed: needed_executors,
            provided: executors_len,
        });
    }
    let state = State {
        processor,
        executors,
        needed_executors,
        // We start events at ID 1, since the instantiation itself is event 0.
        next_event_id: BridgeEventId::start().next(),
        next_action_id: BridgeActionId::start(),
    };
    STATE.save(deps.storage, &state)?;
    Ok(Response::new())
}

#[entry_point]
pub fn execute(deps: DepsMut, _env: Env, info: MessageInfo, msg: ExecuteMsg) -> Result<Response> {
    match msg {
        ExecuteMsg::Regular { keys } => regular(deps, info, keys),
        ExecuteMsg::Signed {
            processor,
            executors,
            payload,
        } => signed(deps, info, processor, executors, payload),
    }
}

fn bridge_event_message_to_response(
    msg: &BridgeEventMessage,
    id: BridgeEventId,
    storage: &mut dyn Storage,
) -> Result<Response> {
    let message = to_json_binary(msg)?;
    EVENTS.save(storage, id, &message)?;
    Ok(Response::new().add_event(
        Event::new("bridge-event")
            .add_attribute("id", id.to_string())
            .add_attribute("message", message.to_string()),
    ))
}

fn regular(deps: DepsMut, info: MessageInfo, keys: Vec<PublicKey>) -> Result<Response> {
    let mut state = STATE.load(deps.storage)?;
    let id = state.next_event_id;
    state.next_event_id.increment();
    STATE.save(deps.storage, &state)?;

    bridge_event_message_to_response(
        &BridgeEventMessage::Regular {
            wallet: info.sender.into_string(),
            funds: info.funds,
            keys,
        },
        id,
        deps.storage,
    )
}

fn signed(
    deps: DepsMut,
    info: MessageInfo,
    processor: SignatureWithRecovery,
    executors: Vec<SignatureWithRecovery>,
    payload: String,
) -> Result<Response> {
    let Payload { id, messages } = from_json(&payload)?;

    let mut state = STATE.load(deps.storage)?;
    if id != state.next_action_id {
        return Err(Error::IncorrectActionId {
            expected: state.next_action_id,
            received: id,
        });
    }

    state.next_action_id.increment();
    let incoming_id = state.next_event_id;
    state.next_event_id.increment();
    STATE.save(deps.storage, &state)?;

    let executors_len = u16::try_from(executors.len()).map_err(Error::TooManyExecutors)?;
    if executors_len < state.needed_executors {
        return Err(Error::InsufficientSignatures {
            needed: state.needed_executors,
            provided: executors_len,
        });
    }

    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let hash = hasher.finalize();

    let processor = signing::validate_signature(deps.api, &hash, &processor)?;

    if processor != state.processor {
        return Err(Error::NonProcessorKey {
            expected: state.processor.into(),
            actual: processor.into(),
        });
    }

    let mut used = vec![];
    for executor in executors {
        let key = signing::validate_signature(deps.api, &hash, &executor)?;
        if !state.executors.contains(&key) {
            return Err(Error::NonExecutorKey { key });
        }
        if used.contains(&key) {
            return Err(Error::DuplicateKey { key });
        }
        used.push(key);
    }

    Ok(bridge_event_message_to_response(
        &BridgeEventMessage::Signed {
            wallet: info.sender.into_string(),
            action_id: id,
        },
        incoming_id,
        deps.storage,
    )?
    .add_messages(messages))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary> {
    match msg {
        QueryMsg::Config {} => {
            let state = STATE.load(deps.storage)?;
            to_json_binary(&state).map_err(Into::into)
        }
        QueryMsg::GetEvent { id } => {
            let resp = match EVENTS.may_load(deps.storage, id)? {
                Some(message) => GetEventResp::Found { message },
                None => GetEventResp::NotFound {},
            };
            to_json_binary(&resp).map_err(Into::into)
        }
    }
}
