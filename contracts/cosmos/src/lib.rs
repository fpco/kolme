mod signing;

use std::num::TryFromIntError;

use cosmwasm_std::{
    entry_point, from_json, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, Event,
    MessageInfo, Response, Storage,
};
use cw_storage_plus::{Item, Map};
use sha2::{Digest, Sha256};
use shared::{
    cosmos::*,
    cryptography::{PublicKey, SignatureWithRecovery},
    types::{BridgeActionId, BridgeEventId, SelfReplace, ValidatorType},
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
    #[error("No approvers provided")]
    NoApproversProvided,
    #[error("Too many approvers provided")]
    TooManyApprovers(TryFromIntError),
    #[error("Need at least {needed} approvers , but only {provided} provided.")]
    InsufficientApprovers { needed: u16, provided: u16 },
    #[error("Incorrect action ID. Expected: {expected}. Received: {received}.")]
    IncorrectActionId {
        expected: BridgeActionId,
        received: BridgeActionId,
    },
    #[error("Insufficient approver signatures provided. Needed: {needed}. Provided: {provided}.")]
    InsufficientSignatures { needed: u16, provided: u16 },
    #[error("Public key {key} is not part of the approver set.")]
    NonApproverKey { key: PublicKey },
    #[error("Duplicate public key provided: {key}.")]
    DuplicateKey { key: PublicKey },
    #[error("Provided signature had the wrong public key. Expected key {expected}. Actually signed with {actual}.")]
    PublicKeyRecoveryMismatch {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },
    #[error("Invalid self-replace. Signed by {signed_by} for validator type {validator_type}. Existing set: {expected:?}. Replacement provided: {replacement}.")]
    InvalidSelfReplace {
        signed_by: Box<PublicKey>,
        replacement: Box<PublicKey>,
        validator_type: ValidatorType,
        expected: String,
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
        approvers,
        needed_approvers,
    }: InstantiateMsg,
) -> Result<Response> {
    if approvers.is_empty() {
        return Err(Error::NoApproversProvided);
    }
    let approvers_len = u16::try_from(approvers.len()).map_err(Error::TooManyApprovers)?;
    if approvers_len < needed_approvers {
        return Err(Error::InsufficientApprovers {
            needed: needed_approvers,
            provided: approvers_len,
        });
    }
    let state = State {
        processor,
        approvers,
        needed_approvers,
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
            approvers,
            payload,
        } => signed(deps, info, processor, approvers, payload),
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

fn regular(deps: DepsMut, info: MessageInfo, keys: Vec<KeyRegistration>) -> Result<Response> {
    let mut hasher = Sha256::new();
    hasher.update(info.sender.as_bytes());

    let hash = hasher.finalize();

    for r in &keys {
        let recovered_key = signing::validate_signature(deps.api, &hash, &r.signature)?;

        if recovered_key != r.key {
            return Err(Error::PublicKeyRecoveryMismatch {
                expected: r.key.into(),
                actual: recovered_key.into(),
            });
        }
    }

    let mut state = STATE.load(deps.storage)?;
    let id = state.next_event_id;
    state.next_event_id.increment();
    STATE.save(deps.storage, &state)?;

    bridge_event_message_to_response(
        &BridgeEventMessage::Regular {
            wallet: info.sender.into_string(),
            funds: info.funds,
            keys: keys.into_iter().map(|x| x.key).collect(),
        },
        id,
        deps.storage,
    )
}

fn signed(
    deps: DepsMut,
    info: MessageInfo,
    processor: SignatureWithRecovery,
    approvers: Vec<SignatureWithRecovery>,
    payload_string: String,
) -> Result<Response> {
    let PayloadWithId { id, action } = from_json(&payload_string)?;

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

    let approvers_len = u16::try_from(approvers.len()).map_err(Error::TooManyApprovers)?;
    if approvers_len < state.needed_approvers {
        return Err(Error::InsufficientSignatures {
            needed: state.needed_approvers,
            provided: approvers_len,
        });
    }

    let mut hasher = Sha256::new();
    hasher.update(&payload_string);
    let hash = hasher.finalize();

    enum ParsedAction {
        Cosmos(Vec<CosmosMsg>),
        SelfReplace {
            validator_type: ValidatorType,
            old: PublicKey,
            new: PublicKey,
        },
    }

    // Determine who the expected processor and approver signatures are.
    // This is tricky because of key rotation. If we're recording a key rotation
    // right now, validate that the signature on the key rotation itself is valid
    // and then trust the new entries.
    let (expected_processor, expected_approvers, action) = match action {
        CosmosAction::Cosmos(messages) => (
            state.processor,
            state.approvers.clone(),
            ParsedAction::Cosmos(messages),
        ),
        CosmosAction::SelfReplace {
            rendered,
            signature,
        } => {
            let hash = Sha256::digest(&rendered);
            let validator = signing::validate_signature(deps.api, &hash, &signature)?;
            let SelfReplace {
                validator_type,
                replacement,
            } = from_json(&rendered)?;
            let mut expected_processor = state.processor;
            let mut expected_approvers = state.approvers.clone();
            match validator_type {
                ValidatorType::Listener => (),
                ValidatorType::Processor => {
                    if validator != state.processor {
                        return Err(Error::InvalidSelfReplace {
                            signed_by: validator.into(),
                            replacement: replacement.into(),
                            validator_type,
                            expected: state.processor.to_string(),
                        });
                    }
                    expected_processor = replacement;
                }
                ValidatorType::Approver => {
                    if !state.approvers.contains(&validator)
                        || state.approvers.contains(&replacement)
                    {
                        return Err(Error::InvalidSelfReplace {
                            signed_by: validator.into(),
                            replacement: replacement.into(),
                            validator_type,
                            expected: format!("{:?}", state.approvers),
                        });
                    }
                    expected_approvers.remove(&validator);
                    expected_approvers.insert(replacement);
                }
            }
            (
                expected_processor,
                expected_approvers,
                ParsedAction::SelfReplace {
                    validator_type,
                    old: validator,
                    new: replacement,
                },
            )
        }
    };

    let processor = signing::validate_signature(deps.api, &hash, &processor)?;

    if processor != expected_processor {
        return Err(Error::PublicKeyRecoveryMismatch {
            expected: state.processor.into(),
            actual: processor.into(),
        });
    }

    let mut used = vec![];
    for approver in approvers {
        let key = signing::validate_signature(deps.api, &hash, &approver)?;
        if !expected_approvers.contains(&key) {
            continue;
        }
        if used.contains(&key) {
            return Err(Error::DuplicateKey { key });
        }
        used.push(key);
    }
    if used.len() < state.needed_approvers as usize {
        return Err(Error::InsufficientApprovers {
            needed: state.needed_approvers,
            provided: used.len() as u16,
        });
    }

    let mut event = bridge_event_message_to_response(
        &BridgeEventMessage::Signed {
            wallet: info.sender.into_string(),
            action_id: id,
        },
        incoming_id,
        deps.storage,
    )?;

    match action {
        ParsedAction::Cosmos(messages) => event = event.add_messages(messages),
        ParsedAction::SelfReplace {
            validator_type,
            old,
            new,
        } => match validator_type {
            ValidatorType::Listener => todo!(),
            ValidatorType::Processor => {
                // Already checked above, but why not
                assert_eq!(state.processor, old);
                state.processor = new;
            }
            ValidatorType::Approver => {
                state.approvers.remove(&old);
                state.approvers.insert(new);
            }
        },
    };
    STATE.save(deps.storage, &state)?;
    Ok(event)
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
