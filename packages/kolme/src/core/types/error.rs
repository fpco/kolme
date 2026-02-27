use crate::core::*;
use kolme_store::KolmeStoreError;
use tokio::sync::watch::error::RecvError;
#[cfg(any(feature = "solana", feature = "cosmwasm", feature = "pass_through"))]
use crate::submitter::SubmitterError;
#[cfg(feature = "cosmwasm")]
use cosmos::error::{AddressError, WalletError};
#[cfg(feature = "cosmwasm")]
use cosmos::{error::BuilderError, CosmosConfigError};
#[cfg(feature = "solana")]
use solana_signature::ParseSignatureError;
#[cfg(feature = "solana")]
use kolme_solana_bridge_client::pubkey::ParsePubkeyError;

#[derive(thiserror::Error, Debug)]
pub enum KolmeError {
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

    #[cfg(feature = "cosmwasm")]
    #[error("Failed to execute signed Cosmos bridge transaction: {0}")]
    CosmosExecutionFailed(#[source] cosmos::Error),

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

    #[cfg(any(feature = "solana", feature = "cosmwasm", feature = "pass_through"))]
    #[error("Submitter error: {0}")]
    Submitter(#[from] SubmitterError),

    #[error("Import/export error: {0}")]
    ImportExport(#[from] KolmeImportExportError),

    #[error("Store error: {0}")]
    StoreError(#[from] KolmeStoreError),

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

    #[cfg(feature = "solana")]
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
    Transaction(TransactionError),

    #[error("Broadcast receive error")]
    BroadcastRecv(#[from] tokio::sync::broadcast::error::RecvError),

    #[cfg(feature = "cosmwasm")]
    #[error("Address error")]
    Address(#[from] AddressError),

    #[error("Action error")]
    ActionError(String),

    #[cfg(feature = "cosmwasm")]
    #[error("Wallet error")]
    Wallet(#[from] WalletError),

    #[cfg(feature = "solana")]
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

    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosConfigError(#[from] CosmosConfigError),

    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    BuilderError(#[from] BuilderError),

    #[error(transparent)]
    AxumError(#[from] axum::Error),

    #[cfg(feature = "solana")]
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

    #[cfg(feature = "solana")]
    #[error("Solana client error")]
    SolanaClient(#[from] solana_client::client_error::ClientError),

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

    #[error("Timed out proposing and awaiting transaction {txhash}")]
    TimeoutProposingTx {
        txhash: TxHash,
        #[source]
        elapsed: tokio::time::error::Elapsed,
    },

    #[error("Current processor pubkey is {pubkey}, but we don't have the matching secret key, we have: {pubkeys:?}")]
    MissingProcessorSecret {
        pubkey: Box<PublicKey>,
        pubkeys: Vec<PublicKey>,
    },

    #[error("Block {0} not found")]
    BlockNotFound(BlockHeight),

    #[error("No blocks in chain")]
    NoBlocksInChain,

    #[error("Invalid chain version: {0}")]
    InvalidChainVersion(String),

    #[error("Chain version {requested} not found, earliest is {earliest}")]
    ChainVersionNotFound { requested: String, earliest: String },

    #[error("Underflow in prev() operation")]
    UnderflowInPrev,

    #[error("Unable to compare chain versions")]
    VersionComparisonFailed,

    #[error("Could not find any block with chain version {0}")]
    BlockNotFoundOnChainVersion(String),

    #[error("Could not find first block for chain version {0}")]
    FirstBlockNotFound(String),

    #[error("Could not find last block for chain version {0}")]
    LastBlockNotFound(String),

    #[error("Timeout waiting for block {0}")]
    BlockTimeout(BlockHeight),

    #[error("Failed to load block {0}")]
    BlockLoadFailed(BlockHeight),

    #[error("Expected block {height} not found during resync")]
    BlockNotFoundDuringResync { height: BlockHeight },

    #[error("Executed height mismatch: expected {expected}, actual {actual}")]
    ExecutedHeight {
        expected: BlockHeight,
        actual: BlockHeight,
    },

    #[error("Executed loads mismatch")]
    ExecutedLoads {
        expected: Vec<BlockDataLoad>,
        actual: Vec<BlockDataLoad>,
    },

    #[error("Framework state hash mismatch")]
    FrameworkStateHash {
        expected: Sha256Hash,
        actual: Sha256Hash,
    },

    #[error("App state hash mismatch")]
    AppStateHash {
        expected: Sha256Hash,
        actual: Sha256Hash,
    },

    #[error("Logs hash mismatch")]
    LogsHash {
        expected: Sha256Hash,
        actual: Sha256Hash,
    },

    #[error("Missing merkle layer {hash} in source store")]
    MissingMerkleLayer { hash: Sha256Hash },

    #[error("Missing app merkle layer {hash}")]
    MissingAppMerkleLayer { hash: Sha256Hash },

    #[error("Unable to mark block {height} as archived: {source}")]
    ArchiveBlockFailed {
        height: BlockHeight,
        source: KolmeStoreError,
    },

    #[error("Missing framework merkle layer {hash}")]
    MissingFrameworkMerkleLayer { hash: Sha256Hash },

    #[error("Missing logs merkle layer {hash}")]
    MissingLogMerkleLayer { hash: Sha256Hash },

    #[error("Listener pubkey not allowed for this event")]
    InvalidListenerPubkey,

    #[error("Listener has already signed this event")]
    DuplicateListenerSignature,

    #[error("Genesis message must be signed by the processor")]
    InvalidGenesisPubkey {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Chain code version mismatch: code={code}, chain={chain}")]
    VersionMismatch {
        code: String,
        chain: String,
        txhash: TxHash,
    },

    #[error("Unexpected extra data loads during block validation")]
    ExtraDataLoads,

    #[error("Genesis message does not match expected value")]
    GenesisMismatch,

    #[error("Not enough approver signatures: needed {needed}, got {actual}")]
    NotEnoughApprovers { needed: u16, actual: usize },

    #[error("Processor approval already exists for this action")]
    ProcessorAlreadyApproved,

    #[error("Duplicate approver signatures found")]
    DuplicateApproverEntries,

    #[error("Cannot approve bridge action with a non-approver pubkey: {pubkey}")]
    NonApproverSignature { pubkey: Box<PublicKey> },

    #[error("Bridge action already approved with pubkey {pubkey}")]
    DuplicateApproverSignature {
        action_id: BridgeActionId,
        chain: ExternalChain,
        pubkey: Box<PublicKey>,
    },

    #[error("Processor signature invalid")]
    InvalidProcessorSignature {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Approver signature invalid: signer {pubkey}")]
    InvalidApproverSignature { pubkey: Box<PublicKey> },

    #[error("Mismatch in prior data loads")]
    DataLoadMismatch,

    #[error("Cannot remove signing key from account")]
    CannotRemoveSigningKey {
        key: Box<PublicKey>,
        account: AccountId,
    },

    #[error("Invalid data load request: expected {expected}, got {actual}. Parse expected: {prev_req}, parse actual: {req}")]
    InvalidDataLoadRequest {
        expected: String,
        actual: String,
        prev_req: String,
        req: String,
    },

    #[error(transparent)]
    Data(#[from] KolmeDataError),

    #[error("Cannot approve missing bridge action {action_id} for chain {chain}")]
    MissingBridgeAction {
        chain: ExternalChain,
        action_id: BridgeActionId,
    },

    #[error("Cannot report on an action when no pending actions are present")]
    NoPendingActionsToReport,

    #[error("No pending action {action_id} found for {chain}")]
    NoPendingActionsOnChain {
        action_id: BridgeActionId,
        chain: ExternalChain,
    },

    #[error("Specified an unknown proposal ID {admin_proposal_id}")]
    UnknownAdminProposalId { admin_proposal_id: AdminProposalId },

    #[error("Mismatched bridge event")]
    MismatchedBridgeEvent,

    #[error("Unexpected bridge event ID")]
    UnexpectedBridgeEventId,

    #[error("Processor mismatch between genesis info and on-chain state")]
    ProcessorMismatch,

    #[error("Listeners mismatch between genesis info and on-chain state")]
    ListenersMismatch,

    #[error("Approvers mismatch between genesis info and on-chain state")]
    ApproversMismatch,

    #[error("Needed listeners mismatch between genesis info and on-chain state")]
    NeededListenersMismatch,

    #[error("Needed approvers mismatch between genesis info and on-chain state")]
    NeededApproversMismatch,

    #[error("Code ID mismatch: expected {expected}, actual {actual}")]
    CodeIdMismatch { expected: u64, actual: u64 },

    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosError(#[from] cosmos::Error),

    #[error("{0}")]
    Other(String),
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
/// Errors that need to be serialized on order to report through Gossip.
pub enum TransactionError {
    #[error("Transaction {txhash} has max height of {max_height}, but proposed block height is {proposed_height}")]
    PastMaxHeight {
        txhash: TxHash,
        max_height: BlockHeight,
        proposed_height: BlockHeight,
    },

    #[error("Invalid nonce provided for pubkey {pubkey}, account {account_id}. Expected: {expected}. Received: {actual}.")]
    InvalidNonce {
        pubkey: Box<PublicKey>,
        account_id: AccountId,
        expected: AccountNonce,
        actual: AccountNonce,
    },

    #[error("{0}")]
    /// Any other errors are stringified here
    //
    /// This variant should not be constructed directly.
    Other(String),
}
