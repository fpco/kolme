mod signing;

use std::collections::BTreeSet;

use cosmwasm_std::{
    entry_point, from_json, to_json_binary, Binary, CosmosMsg, Deps, DepsMut, Env, Event,
    MessageInfo, Response, Storage,
};
use cw2::{get_contract_version, set_contract_version};
use cw_storage_plus::{Item, Map};
use sha2::{Digest, Sha256};
use shared::{
    cosmos::*,
    cryptography::{PublicKey, SignatureWithRecovery},
    types::{
        BridgeActionId, BridgeEventId, SelfReplace, ValidatorSet,
        ValidatorSetError, ValidatorType, KeyRegistration
    },
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
    #[error(transparent)]
    ValidatorSetError { source: ValidatorSetError },
    #[error(transparent)]
    SemverParse { source: semver::Error },
    #[error("mismatched contract migration name (from {saved} to {proposed})")]
    MismatchedContractMigrationName {
        saved: String,
        proposed: &'static str,
    },
    #[error("cannot migrate contract from newer to older (from {saved} to {proposed})")]
    MigrateToOlder {
        saved: String,
        proposed: &'static str,
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

// version info for migration info
const CONTRACT_NAME: &str = "kolme.fpblock.com:bridge";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    InstantiateMsg { set }: InstantiateMsg,
) -> Result<Response> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    set.validate()
        .map_err(|source| Error::ValidatorSetError { source })?;
    if set.approvers.is_empty() {
        return Err(Error::NoApproversProvided);
    }
    let state = State {
        set,
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
        NewSet(ValidatorSet),
    }

    // Determine who the expected processor and approver signatures are.
    // This is tricky because of key rotation. If we're recording a key rotation
    // right now, validate that the signature on the key rotation itself is valid
    // and then trust the new entries.
    let (expected_processor, expected_approvers, needed_approvers, action) = match action {
        CosmosAction::Cosmos(messages) => (
            state.set.processor,
            state.set.approvers.clone(),
            state.set.needed_approvers,
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
            let mut expected_processor = state.set.processor;
            let mut expected_approvers = state.set.approvers.clone();
            match validator_type {
                ValidatorType::Listener => {
                    if !state.set.listeners.contains(&validator)
                        || state.set.listeners.contains(&replacement)
                    {
                        return Err(Error::InvalidSelfReplace {
                            signed_by: validator.into(),
                            replacement: replacement.into(),
                            validator_type,
                            expected: format!("{:?}", state.set.listeners),
                        });
                    }
                }
                ValidatorType::Processor => {
                    if validator != state.set.processor {
                        return Err(Error::InvalidSelfReplace {
                            signed_by: validator.into(),
                            replacement: replacement.into(),
                            validator_type,
                            expected: state.set.processor.to_string(),
                        });
                    }
                    expected_processor = replacement;
                }
                ValidatorType::Approver => {
                    if !state.set.approvers.contains(&validator)
                        || state.set.approvers.contains(&replacement)
                    {
                        return Err(Error::InvalidSelfReplace {
                            signed_by: validator.into(),
                            replacement: replacement.into(),
                            validator_type,
                            expected: format!("{:?}", state.set.approvers),
                        });
                    }
                    expected_approvers.remove(&validator);
                    expected_approvers.insert(replacement);
                }
            }
            (
                expected_processor,
                expected_approvers,
                state.set.needed_approvers,
                ParsedAction::SelfReplace {
                    validator_type,
                    old: validator,
                    new: replacement,
                },
            )
        }
        CosmosAction::NewSet {
            rendered,
            approvals,
        } => {
            let new_set = from_json::<ValidatorSet>(&rendered)?;
            let hash = Sha256::digest(&rendered);

            let mut processor = false;
            let mut listeners = 0;
            let mut approvers = 0;
            let mut used = BTreeSet::new();

            for signature in approvals {
                let validator = signing::validate_signature(deps.api, &hash, &signature)?;
                if used.contains(&validator) {
                    return Err(Error::DuplicateKey { key: validator });
                }
                used.insert(validator);
                if state.set.processor == validator {
                    assert!(!processor);
                    processor = true;
                }
                if state.set.listeners.contains(&validator) {
                    listeners += 1;
                }
                if state.set.approvers.contains(&validator) {
                    approvers += 1;
                }
            }

            let count = if processor { 1 } else { 0 }
                + if listeners >= state.set.needed_listeners {
                    1
                } else {
                    0
                }
                + if approvers >= state.set.needed_approvers {
                    1
                } else {
                    0
                };

            if count < 2 {
                return Err(Error::InsufficientSignatures {
                    needed: 2,
                    provided: count,
                });
            }

            (
                new_set.processor,
                new_set.approvers.clone(),
                new_set.needed_approvers,
                ParsedAction::NewSet(new_set),
            )
        }
    };

    let processor = signing::validate_signature(deps.api, &hash, &processor)?;

    if processor != expected_processor {
        return Err(Error::PublicKeyRecoveryMismatch {
            expected: expected_processor.into(),
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

    if used.len() < needed_approvers as usize {
        return Err(Error::InsufficientApprovers {
            needed: needed_approvers,
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
            ValidatorType::Listener => {
                state.set.listeners.remove(&old);
                state.set.listeners.insert(new);
            }
            ValidatorType::Processor => {
                // Already checked above, but why not
                assert_eq!(state.set.processor, old);
                state.set.processor = new;
            }
            ValidatorType::Approver => {
                state.set.approvers.remove(&old);
                state.set.approvers.insert(new);
            }
        },
        ParsedAction::NewSet(set) => state.set = set,
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

#[entry_point]
pub fn migrate(deps: DepsMut, _env: Env, MigrateMsg {}: MigrateMsg) -> Result<Response> {
    let old_cw2 = get_contract_version(deps.storage)?;
    let old_version: semver::Version = old_cw2
        .version
        .parse()
        .map_err(|source| Error::SemverParse { source })?;
    let new_version: semver::Version = CONTRACT_VERSION
        .parse()
        .map_err(|source| Error::SemverParse { source })?;

    if old_cw2.contract != CONTRACT_NAME {
        Err(Error::MismatchedContractMigrationName {
            saved: old_cw2.contract,
            proposed: CONTRACT_NAME,
        })
    } else if old_version > new_version {
        Err(Error::MigrateToOlder {
            saved: old_cw2.version,
            proposed: CONTRACT_VERSION,
        })
    } else {
        set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
        let response = Response::new()
            .add_attribute("old_contract_name", old_cw2.contract)
            .add_attribute("old_contract_version", old_cw2.version)
            .add_attribute("new_contract_name", CONTRACT_NAME)
            .add_attribute("new_contract_version", CONTRACT_VERSION);
        Ok(response)
    }
}
