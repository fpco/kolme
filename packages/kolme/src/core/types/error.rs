use crate::api_server::KolmeApiError;
use crate::listener::{cosmos::CosmosListenerError, solana::ListenerSolanaError};
use crate::{core::*, submitter::SubmitterError};
use cosmos::error::{AddressError, WalletError};
use cosmos::{error::BuilderError, CosmosConfigError};
use kolme_solana_bridge_client::pubkey::ParsePubkeyError;
use kolme_store::KolmeStoreError;
use solana_signature::ParseSignatureError;
use tokio::sync::watch::error::RecvError;

#[derive(thiserror::Error, Debug)]
pub enum KolmeError {
    #[error("Invalid nonce provided for pubkey {pubkey}, account {account_id}. Expected: {expected}. Received: {actual}.")]
    InvalidNonce {
        pubkey: Box<PublicKey>,
        account_id: AccountId,
        expected: AccountNonce,
        actual: AccountNonce,
    },

    #[error("Already have a bridge contract for {chain:?}, just received another from a listener")]
    BridgeAlreadyDeployed { chain: ExternalChain },

    #[error(
        "Signing public key {signer} is not a member of the {role:?} set and cannot self-replace"
    )]
    NotInValidatorSet {
        signer: Box<PublicKey>,
        role: ValidatorRole,
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
        actual: BlockHash,
        expected: BlockHash,
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

    #[error("Failed to execute signed Cosmos bridge transaction: {0}")]
    CosmosExecutionFailed(#[from] cosmos::Error),

    #[error("API server error")]
    ApiServerError(#[from] std::io::Error),

    #[error("Mismatched genesis info: actual {actual:?}, expected {expected:?}")]
    MismatchedGenesisInfo {
        actual: GenesisInfo,
        expected: GenesisInfo,
    },

    #[error("Identical proposal {id} already exists")]
    DuplicateAdminProposal { id: AdminProposalId },

    #[error("Invalid signature: expected signer {expected}, actual {actual}")]
    InvalidSignature {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Executed block height mismatch: expected {expected}, got {actual}")]
    ExecutedHeightMismatch {
        expected: BlockHeight,
        actual: BlockHeight,
    },

    #[error("Submitter error: {0}")]
    Submitter(#[from] SubmitterError),

    #[error("Import/export error: {0}")]
    ImportExport(#[from] KolmeImportExportError),

    #[error("Core error: {0}")]
    CoreError(#[from] KolmeCoreError),

    #[error("Listener error: {0}")]
    ListenerError(#[from] CosmosListenerError),

    #[error("Execution error: {0}")]
    ExecuteError(#[from] KolmeExecuteError),

    #[error("Execution error: {0}")]
    Execution(#[from] KolmeExecutionError),

    #[error("Store error: {0}")]
    StoreError(#[from] KolmeStoreError),

    #[error("API server error")]
    ApiError(#[from] KolmeApiError),

    #[error(transparent)]
    Secretkey(#[from] SecretKeyError),

    #[error("Transaction already in mempool")]
    TxAlreadyInMempool,

    #[error("Transaction already included in block {0}")]
    TxAlreadyInBlock(BlockHeight),

    #[error("Failed to serialize Solana payload to Borsh")]
    SolanaPayloadSerializationError(#[source] std::io::Error),

    #[error("Failed to build Solana initialization transaction")]
    SolanaInitTxBuildFailed(#[source] std::io::Error),

    #[error("Failed to create Solana pubsub client")]
    SolanaPubsubError(#[from] solana_client::nonblocking::pubsub_client::PubsubClientError),

    #[error("Bridge program {program} hasn't been initialized yet")]
    UninitializedSolanaBridge { program: String },

    #[error("Error deserializing Solana bridge state: {details}")]
    InvalidSolanaBridgeState { details: String },

    #[error("Error deserializing Solana bridge message from logs: {details}")]
    InvalidSolanaBridgeLogMessage { details: String },

    #[error("Start height {start} is greater than {end} height")]
    InvalidBlockHeight {
        start: BlockHeight,
        end: BlockHeight,
    },

    #[error(transparent)]
    Transaction(#[from] TransactionError),

    #[error("Broadcast receive error")]
    BroadcastRecv(#[from] tokio::sync::broadcast::error::RecvError),

    #[error("Solana listener error: {0}")]
    ListenerSolanaError(#[from] ListenerSolanaError),

    #[error("Address error")]
    Address(#[from] AddressError),

    #[error("Action error")]
    ActionError(String),

    #[error("Wallet error")]
    Wallet(#[from] WalletError),

    #[error("Parse pubkey error")]
    ParsePubkey(#[from] ParsePubkeyError),

    #[error("CoreState error")]
    CoreState(#[from] CoreStateError),

    #[error("Asset error")]
    Asset(#[from] AssetError),

    #[error("Accounts error")]
    Accounts(#[from] AccountsError),

    #[error("Validator set error")]
    ValidatorSet(#[from] ValidatorSetError),

    #[error("Public key error")]
    PublicKey(#[from] PublicKeyError),

    #[error("Merkle serialization error")]
    MerkleSerial(#[from] MerkleSerialError),

    #[error("JSON error")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    RecvError(#[from] RecvError),

    #[error(transparent)]
    CosmosConfigError(#[from] CosmosConfigError),

    #[error(transparent)]
    BuilderError(#[from] BuilderError),

    #[error(transparent)]
    AxumError(#[from] axum::Error),

    #[error(transparent)]
    ParseSignatureError(#[from] ParseSignatureError),

    #[error("TryFromInt error")]
    TryFromInt(#[from] std::num::TryFromIntError),

    #[error("Base64 decode error")]
    Base64(#[from] base64::DecodeError),

    #[error("WebSocket stream terminated")]
    WebSocketClosed,

    #[error("WebSocket error")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("Solana client error")]
    SolanaClient(#[from] solana_client::client_error::ClientError),

    #[error("{0}")]
    Other(String),

    #[error("Latest block height is {height}, but it wasn't found in the data store")]
    BlockMissingInStore { height: BlockHeight },

    #[error("Emit latest block: no blocks available")]
    NoBlocksAvailable,

    #[error("Block signed by invalid processor: expected {expected}, got {actual}")]
    InvalidBlockProcessorSignature {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Transaction signed by invalid key: expected {expected}, got {actual}")]
    InvalidTransactionSignature {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Genesis transaction format invalid")]
    InvalidGenesisTransaction,

    #[error("Failed to verify transaction signature")]
    SignatureVerificationFailed,

    #[error("Overflow while depositing asset {asset_id}, amount == {amount}")]
    OverflowWhileDepositing { asset_id: AssetId, amount: Decimal },

    #[error("Insufficient funds while withdrawing asset {asset_id}, amount == {amount}")]
    InsufficientFundsWhileWithdrawing { asset_id: AssetId, amount: Decimal },
    #[error("Unsupported asset ID")]
    UnsupportedAssetId,
}

impl KolmeError {
    pub fn other<E: std::fmt::Display>(e: E) -> Self {
        Self::Other(e.to_string())
    }
}

impl<T> From<ProposeTransactionError<T>> for KolmeError {
    fn from(e: ProposeTransactionError<T>) -> Self {
        match e {
            ProposeTransactionError::InMempool => KolmeError::TxAlreadyInMempool,

            ProposeTransactionError::InBlock(block) => KolmeError::TxAlreadyInBlock(block.height()),

            ProposeTransactionError::Failed(failed) => {
                KolmeError::Transaction(failed.message.as_inner().error.clone())
            }
        }
    }
}

#[derive(thiserror::Error, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TransactionError {
    #[error("Store error: {0}")]
    StoreError(String),

    #[error("Core error: {0}")]
    CoreError(String),

    #[error("Block with height {height} in database with different hash {existing}, trying to add {adding}")]
    ConflictingBlockInDb {
        height: u64,
        adding: Sha256Hash,
        existing: Sha256Hash,
    },

    #[error("Executed height mismatch: expected {expected}, got {actual}")]
    ExecutedHeightMismatch {
        expected: BlockHeight,
        actual: BlockHeight,
    },

    #[error("Transaction {txhash} has max height of {max_height}, but proposed block height is {proposed_height}")]
    PastMaxHeight {
        txhash: TxHash,
        max_height: BlockHeight,
        proposed_height: BlockHeight,
    },

    #[error("Timed out proposing transaction {txhash}")]
    TimeoutProposingTx { txhash: TxHash },

    #[error("Invalid nonce provided for pubkey {pubkey}, account {account_id}. Expected: {expected}. Received: {actual}.")]
    InvalidNonce {
        pubkey: Box<PublicKey>,
        account_id: AccountId,
        expected: AccountNonce,
        actual: AccountNonce,
    },
}

impl From<KolmeError> for TransactionError {
    fn from(err: KolmeError) -> Self {
        match err {
            KolmeError::ExecutedHeightMismatch { expected, actual } => {
                TransactionError::ExecutedHeightMismatch { expected, actual }
            }
            KolmeError::StoreError(e) => TransactionError::StoreError(e.to_string()),
            KolmeError::InvalidNonce {
                pubkey,
                account_id,
                expected,
                actual,
            } => TransactionError::InvalidNonce {
                pubkey: pubkey.clone(),
                account_id,
                expected,
                actual,
            },
            _ => TransactionError::CoreError(err.to_string()),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeExecutionError {
    #[error("Mismatched bridge event")]
    MismatchedBridgeEvent,

    #[error("Unexpected bridge event ID")]
    UnexpectedBridgeEventId,
}
