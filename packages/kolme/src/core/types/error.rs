use crate::core::*;
use cosmos::error::{AddressError, WalletError};
use kolme_solana_bridge_client::pubkey::ParsePubkeyError;
use kolme_store::KolmeStoreError;
use std::num::TryFromIntError;
use tokio::sync::broadcast::error::RecvError;

#[macro_export]
macro_rules! kolme_ensure {
    ($cond:expr $(,)?) => {
        if !$cond {
            return Err($crate::core::KolmeError::Other(format!(
                "Condition failed: {}",
                stringify!($cond)
            )));
        }
    };

    ($cond:expr, $($arg:tt)*) => {
        if !$cond {
            return Err($crate::core::KolmeError::Other(format!($($arg)*)));
        }
    };
}

#[derive(thiserror::Error, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum KolmeError {
    #[error("Invalid nonce provided for pubkey {pubkey}, account {account_id}. Expected: {expected}. Received: {actual}.")]
    InvalidNonce {
        pubkey: Box<PublicKey>,
        account_id: AccountId,
        expected: AccountNonce,
        actual: AccountNonce,
    },
    /// A transaction had a max height set, but the chain has already moved past that height.
    ///
    /// The `max_height` field represents the max height specified by the client.
    /// `proposed_height` is the height at which we tried to add this transaction.
    #[error("Transaction {txhash} has max height of {max_height}, but proposed block height is {proposed_height}")]
    PastMaxHeight {
        txhash: TxHash,
        max_height: BlockHeight,
        proposed_height: BlockHeight,
    },

    #[error("Already have a bridge contract for {chain:?}, just received another from a listener")]
    BridgeAlreadyDeployed { chain: ExternalChain },

    #[error(
        "Signing public key {signer} is not a member of the {role} set and cannot self-replace"
    )]
    NotInValidatorSet {
        signer: Box<PublicKey>,
        role: String,
    },

    #[error("Signing public key {signer} is not the current processor and cannot self-replace")]
    NotProcessor { signer: Box<PublicKey> },

    #[error("Validator self-replace signature doesn't match the current validator pubkey")]
    InvalidSelfReplaceSigner,

    #[error("Tried to add block with height {received}, but next expected height is {expected}")]
    UnexpectedBlockHeight {
        received: BlockHeight,
        expected: BlockHeight,
    },

    #[error("Tried to add block with height {height}, but it's already present in the store")]
    BlockAlreadyExists { height: BlockHeight },

    #[error("Received block signed by processor {actual_processor}, but the real processor is {expected_processor}")]
    InvalidBlockProcessor {
        expected_processor: Box<PublicKey>,
        actual_processor: Box<PublicKey>,
    },

    #[error("Unable to migrate contract for chain {chain:?}: contract isn't deployed")]
    ContractNotDeployed { chain: ExternalChain },

    #[error("Already have a deployed contract on {chain:?}")]
    ContractAlreadyDeployed { chain: ExternalChain },

    #[cfg(feature = "pass_through")]
    #[error("No wait for pass-through contract is expected")]
    UnexpectedPassThroughContract,

    #[error("Persistent task exited unexpectedly")]
    PersistentTaskExited,

    #[error("Task exited with an error: {error}")]
    TaskErrored { error: String },

    #[error("Task panicked: {details}")]
    TaskPanicked { details: String },

    #[error("Trying to configure a Cosmos contract as a Solana bridge")]
    CosmosBridgeConfiguredAsSolana,

    #[error("Trying to configure a Solana program as a Cosmos bridge")]
    SolanaBridgeConfiguredAsCosmos,

    #[cfg(feature = "pass_through")]
    #[error("Multiple pass-through bridges are not supported")]
    MultiplePassThroughBridgesUnsupported,

    #[cfg(feature = "pass_through")]
    #[error("Pass-through bridge can't require Cosmos or Solana bridge contract")]
    InvalidPassThroughBridgeType,

    #[error("Expected exactly one message in the first block, but found a different number")]
    InvalidGenesisMessageCount,

    #[error("Invalid messages in first block")]
    InvalidFirstBlockMessageType,

    #[error("Listener panicked: {details}")]
    ListenerPanicked { details: String },

    #[error("Block parent mismatch: actual {actual}, expected {expected}")]
    BlockParentMismatch {
        actual: Box<BlockHash>,
        expected: Box<BlockHash>,
    },

    #[error("Action ID mismatch: expected {expected}, found {found}")]
    ActionIdMismatch {
        expected: BridgeActionId,
        found: BridgeActionId,
    },

    #[error("Validator {signer} already approved proposal {proposal_id}")]
    AlreadyApprovedProposal {
        signer: PublicKey,
        proposal_id: AdminProposalId,
    },

    #[error("Conflicting block in DB at height {height} with hash {hash}")]
    ConflictingBlockInDb { height: u64, hash: Sha256Hash },

    #[error("Failed to execute signed Cosmos bridge transaction: {details}")]
    CosmosExecutionFailed { details: String },

    #[error("Timed out proposing and awaiting transaction {txhash}: {details}")]
    TimeoutProposingTx { txhash: TxHash, details: String },

    #[error("API server error: {details}")]
    ApiServerError { details: String },

    #[error("Execution error: {0}")]
    Execution(#[from] KolmeExecutionError),

    #[error("Store error: {0}")]
    StoreError(#[from] KolmeStoreError),

    #[error("Failed to serialize Solana payload to Borsh: {details}")]
    SolanaPayloadSerializationError { details: String },

    #[error("Failed to build Solana initialization transaction: {details}")]
    SolanaInitTxBuildFailed { details: String },

    #[error("Solana submitter failed to execute signed transaction: {details}")]
    SolanaSignedTxExecutionFailed { details: String },

    #[error("Failed to create Solana pubsub client: {0}")]
    SolanaPubsubError(String),

    #[error("Bridge program {program} hasn't been initialized yet")]
    UninitializedSolanaBridge { program: String },

    #[error("Error deserializing Solana bridge state: {details}")]
    InvalidSolanaBridgeState { details: String },

    #[error("Error deserializing Solana bridge message from logs: {details}")]
    InvalidSolanaBridgeLogMessage { details: String },

    #[error("{0}")]
    Other(String),
}

// CREATE A GENERIC, MACRO OR SOMETHING WITH SIMILAR BEHAVIOR

impl From<CoreStateError> for KolmeError {
    fn from(e: CoreStateError) -> Self {
        KolmeError::Other(format!("CoreState error: {e}"))
    }
}

impl From<AssetError> for KolmeError {
    fn from(e: AssetError) -> Self {
        KolmeError::Other(format!("Asset error: {e}"))
    }
}

impl From<AccountsError> for KolmeError {
    fn from(e: AccountsError) -> Self {
        KolmeError::Other(format!("Accounts error: {e}"))
    }
}

impl From<anyhow::Error> for KolmeError {
    fn from(e: anyhow::Error) -> Self {
        KolmeError::Other(format!("Error from Anyhow: {e}"))
    }
}

impl From<std::io::Error> for KolmeError {
    fn from(e: std::io::Error) -> Self {
        KolmeError::Other(format!("std::io::Error Error: {e}"))
    }
}

impl From<ValidatorSetError> for KolmeError {
    fn from(e: ValidatorSetError) -> Self {
        KolmeError::Other(format!("Validator set error: {e}"))
    }
}

impl From<PublicKeyError> for KolmeError {
    fn from(e: PublicKeyError) -> Self {
        KolmeError::Other(format!("Public key error: {e}"))
    }
}

impl From<MerkleSerialError> for KolmeError {
    fn from(e: MerkleSerialError) -> Self {
        KolmeError::Other(format!("Merkle serialization error: {e}"))
    }
}

impl From<ParsePubkeyError> for KolmeError {
    fn from(e: ParsePubkeyError) -> Self {
        KolmeError::Other(format!("Parse public key error: {e}"))
    }
}

impl From<AddressError> for KolmeError {
    fn from(e: AddressError) -> Self {
        KolmeError::Other(format!("Address error: {e}"))
    }
}

impl From<TryFromIntError> for KolmeError {
    fn from(e: TryFromIntError) -> Self {
        KolmeError::Other(format!("TryFromInt error: {e}"))
    }
}

impl From<serde_json::Error> for KolmeError {
    fn from(e: serde_json::Error) -> Self {
        KolmeError::Other(format!("Serde JSON error: {e}"))
    }
}

impl From<RecvError> for KolmeError {
    fn from(e: RecvError) -> Self {
        KolmeError::Other(format!("Recv error: {e}"))
    }
}

impl From<solana_client::client_error::ClientError> for KolmeError {
    fn from(e: solana_client::client_error::ClientError) -> Self {
        KolmeError::Other(e.to_string())
    }
}

impl From<base64::DecodeError> for KolmeError {
    fn from(e: base64::DecodeError) -> Self {
        KolmeError::Other(format!("Base64 decode error: {e}"))
    }
}

impl From<WalletError> for KolmeError {
    fn from(e: WalletError) -> Self {
        KolmeError::Other(format!("Wallet error: {e}"))
    }
}

impl From<cosmos::Error> for KolmeError {
    fn from(e: cosmos::Error) -> Self {
        KolmeError::Other(format!("Cosmos error: {e}"))
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for KolmeError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        KolmeError::Other(format!("WebSocket error: {e}"))
    }
}

#[derive(thiserror::Error, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum KolmeExecutionError {
    #[error("Mismatched bridge event")]
    MismatchedBridgeEvent,

    #[error("Unexpected bridge event ID")]
    UnexpectedBridgeEventId,
}
