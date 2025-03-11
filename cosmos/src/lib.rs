use std::num::TryFromIntError;

use cosmwasm_std::{
    entry_point, from_json, to_json_binary, Api, Binary, Coin, CosmosMsg, Deps, DepsMut, Env,
    Event, HexBinary, MessageInfo, Response,
};
use cw_storage_plus::Item;
use sha2::{Digest, Sha256};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Cosmwasm {
        #[from]
        source: cosmwasm_std::StdError,
    },
    #[error(transparent)]
    Verification {
        #[from]
        source: cosmwasm_std::VerificationError,
    },
    #[error("No executors provided")]
    NoExecutorsProvided,
    #[error("Too many executors provided")]
    TooManyExecutors(TryFromIntError),
    #[error("Need at least {needed} executors, but only {provided} provided.")]
    InsufficientExecutors { needed: u16, provided: u16 },
    #[error("Incorrect outgoing ID. Expected: {expected}. Received: {received}.")]
    IncorrectOutgoingId { expected: u32, received: u32 },
    #[error("Insufficient executor signatures provided. Needed: {needed}. Provided: {provided}.")]
    InsufficientSignatures { needed: u16, provided: u16 },
    #[error("Invalid signature {sig} for public key {key}.")]
    InvalidSignature { sig: HexBinary, key: HexBinary },
    #[error("Public key {key} is not part of the executor set.")]
    NonExecutorKey { key: HexBinary },
    #[error("Duplicate public key provided: {key}.")]
    DuplicateKey { key: HexBinary },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
struct State {
    processor: HexBinary,
    executors: Vec<HexBinary>,
    needed_executors: u16,
    next_outgoing_id: u32,
    next_incoming_id: u32,
}

const STATE: Item<State> = Item::new("s");

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct InstantiateMsg {
    /// Public key for the processor
    pub processor: HexBinary,
    /// Public keys of all executors
    pub executors: Vec<HexBinary>,
    /// How many executors are needed to execute a message
    pub needed_executors: u16,
}

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
        next_outgoing_id: 0,
        next_incoming_id: 0,
    };
    STATE.save(deps.storage, &state)?;
    Ok(Response::new())
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// General purpose message for regular interaction.
    ///
    /// You can attach any native funds to this message to perform a deposit for the sending account.
    Regular {
        /// Any new public keys to associate with the sending wallet's account.
        keys: Vec<HexBinary>,
    },
    /// Submit a message signed by the executors and the processor
    ///
    /// Note that this message itself is not permissioned, so that anyone can relay signed messages from the chain.
    Signed {
        /// Monotonically increasing ID to ensure messages are sent in the correct order.
        id: u32,
        /// Signature from the processor
        processor: HexBinary,
        /// Signatures from the executors
        executors: Vec<ExecutorSignature>,
        /// The raw payload to execute
        payload: Binary,
    },
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ExecutorSignature {
    pub key: HexBinary,
    pub sig: HexBinary,
}

#[entry_point]
pub fn execute(deps: DepsMut, _env: Env, info: MessageInfo, msg: ExecuteMsg) -> Result<Response> {
    match msg {
        ExecuteMsg::Regular { keys } => regular(deps, info, keys),
        ExecuteMsg::Signed {
            id,
            processor,
            executors,
            payload,
        } => signed(deps, info, id, processor, executors, payload),
    }
}

#[derive(serde::Serialize)]
enum Message {
    Regular {
        wallet: String,
        funds: Vec<Coin>,
        keys: Vec<HexBinary>,
    },
    Signed {
        wallet: String,
        outgoing_id: u32,
    },
}

impl Message {
    fn to_response(&self, id: u32) -> Result<Response> {
        Ok(Response::new().add_event(
            Event::new("outgoing")
                .add_attribute("id", id.to_string())
                .add_attribute("message", to_json_binary(self)?.to_string()),
        ))
    }
}

fn regular(deps: DepsMut, info: MessageInfo, keys: Vec<HexBinary>) -> Result<Response> {
    let mut state = STATE.load(deps.storage)?;
    let id = state.next_incoming_id;
    state.next_incoming_id += 1;
    STATE.save(deps.storage, &state)?;

    Message::Regular {
        wallet: info.sender.into_string(),
        funds: info.funds,
        keys,
    }
    .to_response(id)
}

fn signed(
    deps: DepsMut,
    info: MessageInfo,
    id: u32,
    processor: HexBinary,
    executors: Vec<ExecutorSignature>,
    payload: Binary,
) -> Result<Response> {
    let mut state = STATE.load(deps.storage)?;
    if id != state.next_outgoing_id {
        return Err(Error::IncorrectOutgoingId {
            expected: state.next_outgoing_id,
            received: id,
        });
    }
    state.next_outgoing_id += 1;
    let incoming_id = state.next_incoming_id;
    state.next_incoming_id += 1;
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

    validate_signature(
        deps.api,
        &hash,
        state.processor.as_slice(),
        processor.as_slice(),
    )?;

    let mut used = vec![];
    for ExecutorSignature { key, sig } in executors {
        if !state.executors.contains(&key) {
            return Err(Error::NonExecutorKey { key });
        }
        if used.contains(&key) {
            return Err(Error::DuplicateKey { key });
        }
        validate_signature(deps.api, &hash, key.as_slice(), sig.as_slice())?;
        used.push(key);
    }

    let msgs = from_json::<Vec<CosmosMsg>>(&payload)?;
    Ok(Message::Signed {
        wallet: info.sender.into_string(),
        outgoing_id: id,
    }
    .to_response(incoming_id)?
    .add_messages(msgs))
}

fn validate_signature(api: &dyn Api, hash: &[u8], key: &[u8], sig: &[u8]) -> Result<()> {
    if api.secp256k1_verify(hash, sig, key)? {
        Ok(())
    } else {
        Err(Error::InvalidSignature {
            sig: HexBinary::from(sig),
            key: HexBinary::from(key),
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, _msg: QueryMsg) -> Result<Binary> {
    let state = STATE.load(deps.storage)?;
    to_json_binary(&state).map_err(|e| e.into())
}
