use crate::core::*;
use crate::listener::cosmos::KolmeListenerError;
use cosmos::error::{AddressError, WalletError};
use kolme_solana_bridge_client::pubkey::ParsePubkeyError;
use kolme_store::KolmeStoreError;
use std::num::TryFromIntError;
use tokio::sync::broadcast::error::RecvError;

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

    #[error("Executed block height mismatch: expected {expected}, got {actual}")]
    ExecutedHeightMismatch {
        expected: BlockHeight,
        actual: BlockHeight,
    },

    #[error("Core error: {0}")]
    CoreError(#[from] KolmeCoreError),

    #[error("Listener error: {0}")]
    ListenerError(#[from] KolmeListenerError),

    #[error("Execution error: {0}")]
    ExecuteError(#[from] KolmeExecuteError),

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

macro_rules! impl_from_to_other {
    ($($source:ty => $msg:expr),+ $(,)?) => {
        $(
            impl From<$source> for KolmeError {
                fn from(e: $source) -> Self {
                    KolmeError::Other(format!($msg, e))
                }
            }
        )+
    };
}

impl_from_to_other! {
    AddressError => "Address error: {}",
    WalletError => "Wallet error: {}",
    ParsePubkeyError => "Parse pubkey error: {}",
    CoreStateError => "CoreState error: {}",
    AssetError => "Asset error: {}",
    AccountsError => "Accounts error: {}",
    ValidatorSetError => "Validator set error: {}",
    PublicKeyError => "Public key error: {}",
    MerkleSerialError => "Merkle serialization error: {}",
    TryFromIntError => "TryFromInt error: {}",
    serde_json::Error => "JSON error: {}",
    RecvError => "Recv error: {}",
    cosmos::Error => "Cosmos error: {}",
    base64::DecodeError => "Base64 decode error: {}",
    solana_client::client_error::ClientError => "Solana client error: {}",
    tokio_tungstenite::tungstenite::Error => "WebSocket error: {}",
    std::io::Error => "IO error: {}",
}

impl From<anyhow::Error> for KolmeError {
    fn from(e: anyhow::Error) -> Self {
        if let Some(inner) = e.downcast_ref::<KolmeError>() {
            return inner.clone();
        }
        KolmeError::Other(format!("Error from Anyhow: {e}"))
    }
}

#[derive(thiserror::Error, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum KolmeExecutionError {
    #[error("Mismatched bridge event")]
    MismatchedBridgeEvent,

    #[error("Unexpected bridge event ID")]
    UnexpectedBridgeEventId,
}
