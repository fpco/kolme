use crate::core::*;
#[cfg(any(feature = "solana", feature = "cosmwasm", feature = "pass_through"))]
use crate::submitter::SubmitterError;
//@@@ REDUCE IMPORTS FOR THESE ERRORS; JUST REFER BY QUALIFIED NAME?
#[cfg(feature = "cosmwasm")]
use cosmos::error::{AddressError, WalletError};
#[cfg(feature = "cosmwasm")]
use cosmos::{error::BuilderError, CosmosConfigError};
#[cfg(feature = "solana")]
use kolme_solana_bridge_client::pubkey::ParsePubkeyError;
use kolme_store::KolmeStoreError;
#[cfg(feature = "solana")]
use solana_signature::ParseSignatureError;

#[derive(thiserror::Error, Debug)]
pub enum KolmeError {
    #[error("Already have a bridge contract for {chain:?}, just received another from a listener")]
    BridgeAlreadyDeployed { chain: ExternalChain },

    #[error(
        "Received a listener message for bridge event ID {event_id} on {chain}, but provided pubkey {pubkey} is not part of the listener set {listener_set:?}"
    )]
    MessagePubkeyNotInValidatorSet {
        event_id: BridgeEventId,
        chain: ExternalChain,
        pubkey: Box<PublicKey>,
        listener_set: BTreeSet<PublicKey>,
    },

    #[error(
        "Signing public key {signer} is not a member of the {role:?} set and cannot self-replace"
    )]
    SigningKeyNotInValidatorSet {
        signer: Box<PublicKey>,
        role: ValidatorRole,
    },

    #[error("Signing public key {signer} is not the current processor and cannot self-replace")]
    NotCurrentProcessor { signer: Box<PublicKey> },

    #[error("Validator self-replace signature doesn't match the current validator pubkey: expected {expected}, actual {actual}")]
    InvalidSelfReplaceSigner {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Tried to add block with height {received}, but next expected height is {expected}")]
    UnexpectedBlockHeight {
        received: BlockHeight,
        expected: BlockHeight,
    },

    #[error("Tried to add block with height {height}, but it's already present in the store")]
    BlockAlreadyExists { height: BlockHeight },

    #[error("Latest block was signed by {pubkey}, but processor is {processor}")]
    LatestInvalidBlockProcessor {
        pubkey: Box<PublicKey>,
        processor: Box<PublicKey>,
    },

    #[error("Received block signed by processor {actual_processor}, but the real processor is {expected_processor}")]
    ReceivedInvalidBlockProcessor {
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

    #[error("Task exited with an error: {0}")]
    TaskErrored(#[source] Box<KolmeError>),

    #[error("Task panicked: {0}")]
    TaskPanicked(#[source] tokio::task::JoinError),

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
    ListenerPanicked { details: tokio::task::JoinError },

    #[error("Tried to add block height {height}, but actual parent has block hash {actual_parent} and block specifies {block_parent}")]
    BlockParentMismatch {
        height: BlockHeight,
        actual_parent: BlockHash,
        block_parent: BlockHash,
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
    CosmosExecutionFailed(#[source] Box<cosmos::Error>),

    #[error("API server error")]
    ApiServerError(#[from] std::io::Error),

    #[error("Mismatched genesis info: actual {actual:?}, expected {expected:?}")]
    MismatchedGenesisInfo {
        actual: Box<GenesisInfo>,
        expected: Box<GenesisInfo>,
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

    #[error("Error serializing Solana bridge payload: {0}")]
    SolanaPayloadSerializationError(#[source] std::io::Error),

    #[error("Failed to build Solana initialization transaction")]
    SolanaInitTxBuildFailed(#[source] std::io::Error),

    #[cfg(feature = "solana")]
    #[error(transparent)]
    SolanaPubsubClientError(Box<solana_client::nonblocking::pubsub_client::PubsubClientError>),

    #[error("Bridge program {program} hasn't been initialized yet")]
    UninitializedSolanaBridge { program: String },

    #[error("Error deserializing Solana bridge state: {0}")]
    InvalidSolanaBridgeState(#[source] std::io::Error),

    #[error("Error deserializing Solana bridge message from logs: {0}")]
    InvalidSolanaBridgeLogMessage(std::io::Error),

    //@@@ MAYBE MOVE THIS INTO TransactionError SINCE IT WAS IN THE ORIGINAL KolmeError
    #[error("Start height {start} is greater than {end} height")]
    InvalidBlockHeight {
        start: BlockHeight,
        end: BlockHeight,
    },

    #[error(transparent)]
    Transaction(TransactionError),

    #[error(transparent)]
    BroadcastRecvError(#[from] tokio::sync::broadcast::error::RecvError),

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

    #[error(transparent)]
    AccountsError(#[from] AccountsError),

    #[error("Validator set error")]
    ValidatorSet(#[from] ValidatorSetError),

    #[error(transparent)]
    PublicKeyError(#[from] PublicKeyError),

    #[error("Merkle serialization error")]
    MerkleSerial(#[from] MerkleSerialError),

    #[error("JSON error")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error(transparent)]
    WatchRecvError(#[from] tokio::sync::watch::error::RecvError),

    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosConfigError(Box<CosmosConfigError>),

    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    BuilderError(Box<BuilderError>),

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
    WebSocket(#[source] Box<tokio_tungstenite::tungstenite::Error>),

    #[cfg(feature = "solana")]
    #[error("Solana client error")]
    SolanaClient(#[source] Box<solana_client::client_error::ClientError>),

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

    //@@@ #[error("Failed to verify transaction signature")]
    //@@@ SignatureVerificationFailed,
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

    #[error("Invalid version from response: {0}")]
    InvalidChainVersionFromResponse(String),

    #[error("Chain version {requested} not found, earliest is {earliest}")]
    ChainVersionNotFound { requested: String, earliest: String },

    #[error("Underflow in prev")]
    UnderflowInPrev,

    #[error("Not able to compare version")]
    VersionComparisonFailed,

    #[error("Could not find a block with chain version {0}")]
    BlockNotFoundOnChainVersion(String),

    #[error("Could not find first block for chain version {0}")]
    FirstBlockNotFound(String),

    #[error("Could not find last block for chain version {0}")]
    LastBlockNotFound(String),

    #[error("Timeout waiting for block {height}: {error}")]
    BlockTimeout {
        height: BlockHeight,
        #[source]
        error: tokio::time::error::Elapsed,
    },

    #[error("Failed to load block {height}: {error}")]
    BlockLoadFailed {
        height: BlockHeight,
        #[source]
        error: Box<KolmeError>,
    },

    #[error("Expected block {height} not found during resync")]
    BlockNotFoundDuringResync { height: BlockHeight },

    #[error("Executed height mismatch: expected {expected}, actual {actual}")]
    ExecutedHeight {
        expected: BlockHeight,
        actual: BlockHeight,
    },

    #[error("Executed loads mismatch: expected {expected:?}, actual {actual:?}")]
    ExecutedLoads {
        expected: Vec<BlockDataLoad>,
        actual: Vec<BlockDataLoad>,
    },

    #[error("Framework state hash mismatch: expected {expected}, actual {actual}")]
    FrameworkStateHash {
        expected: Sha256Hash,
        actual: Sha256Hash,
    },

    #[error("App state hash mismatch: expected {expected}, actual {actual}")]
    AppStateHash {
        expected: Sha256Hash,
        actual: Sha256Hash,
    },

    #[error("Logs hash mismatch: expected {expected}, actual {actual}")]
    LogsHash {
        expected: Sha256Hash,
        actual: Sha256Hash,
    },

    #[error("Missing merkle layer {hash} in source store")]
    MissingMerkleLayer { hash: Sha256Hash },

    #[error("App state {app_state} not written to Merkle store")]
    AppStateNotWritten { app_state: Sha256Hash },

    #[error("Unable to mark block {height} as archived: {source}")]
    ArchiveBlockFailed {
        height: BlockHeight,
        #[source]
        source: KolmeStoreError,
    },

    #[error("Framework state {framework_state} not written to Merkle store")]
    FrameworkStateNotWritten { framework_state: Sha256Hash },

    #[error("Logs {logs} not written to Merkle store")]
    MissingLogMerkleLayer { logs: Sha256Hash },

    #[error("Listener pubkey not allowed for this event")]
    InvalidListenerPubkey,

    #[error("Listener has already signed this event")]
    DuplicateListenerSignature,

    #[error(
        "Genesis message must be signed by the processor: actual {actual}, expected {expected}"
    )]
    InvalidGenesisPubkey {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Cannot execute transaction {txhash}, current code version is {code}, but chain is running {chain}")]
    VersionMismatch {
        code: String,
        chain: String,
        txhash: TxHash,
    },

    #[error("Unexpected extra data loads during block validation")]
    ExtraDataLoads,

    #[error(
        "Genesis message does not match expected value: actual {actual:?}, expected {expected:?}"
    )]
    GenesisMismatch {
        expected: Box<GenesisInfo>,
        actual: Box<GenesisInfo>,
    },

    #[error("Not enough approver signatures: needed {needed}, got {actual}")]
    NotEnoughApprovers { needed: u16, actual: usize },

    #[error("Processor approval already exists for this action")]
    ProcessorAlreadyApproved,

    #[error("Duplicate approver signatures found: actual {actual}, expected {expected}")]
    DuplicateApproverEntries { actual: usize, expected: usize },

    #[error("Cannot approve bridge action with a non-approver pubkey: {pubkey}")]
    NonApproverSignature { pubkey: Box<PublicKey> },

    #[error("Cannot approve bridge action ID {action_id} for chain {chain} with already-used public key {pubkey}")]
    DuplicateApproverSignature {
        action_id: BridgeActionId,
        chain: ExternalChain,
        pubkey: Box<PublicKey>,
    },

    #[error("Processor signature invalid: expected {expected}, actual {actual}")]
    InvalidProcessorSignature {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Approver signature invalid: signer {pubkey}")]
    InvalidApproverSignature { pubkey: Box<PublicKey> },

    #[error("Incorrect number of data loads")]
    IncorrectDataLoads,

    #[error("Cannot remove public key {key} from account {account} with a transaction signed by the same key")]
    CannotRemoveSigningKey {
        key: Box<PublicKey>,
        account: AccountId,
    },

    #[error("Invalid data load request: expected {expected}, got {actual}")]
    InvalidDataLoadRequest { expected: String, actual: String },

    #[error(transparent)]
    Data(#[from] KolmeDataError),

    #[error("Cannot approve missing bridge action {action_id} for chain {chain}")]
    CannotApproveMissingBridgeAction {
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

    #[error("Mismatched bridge event: actual {actual:?}, expected {expected:?}")]
    MismatchedBridgeEvent {
        actual: Box<BridgeEvent>,
        expected: Box<BridgeEvent>,
    },

    #[error("Unexpected bridge event ID: actual {actual}, expected {expected}")]
    UnexpectedBridgeEventId {
        actual: BridgeEventId,
        expected: BridgeEventId,
    },

    #[error("Processor mismatch between genesis info and on-chain state: expected {expected}, actual {actual}")]
    ProcessorMismatch {
        expected: Box<PublicKey>,
        actual: Box<PublicKey>,
    },

    #[error("Listeners mismatch between genesis info and on-chain state: expected {expected:?}, actual {actual:?}")]
    ListenersMismatch {
        expected: BTreeSet<PublicKey>,
        actual: BTreeSet<PublicKey>,
    },

    #[error("Approvers mismatch between genesis info and on-chain state: expected {expected:?}, actual {actual:?}")]
    ApproversMismatch {
        expected: BTreeSet<PublicKey>,
        actual: BTreeSet<PublicKey>,
    },

    #[error("Needed listeners mismatch between genesis info and on-chain state: expected {expected}, actual {actual}")]
    NeededListenersMismatch { expected: u16, actual: u16 },

    #[error("Needed approvers mismatch between genesis info and on-chain state: expected {expected}, actual {actual}")]
    NeededApproversMismatch { expected: u16, actual: u16 },

    #[error("Code ID mismatch, expected {expected}, but {contract} has {actual}")]
    CodeIdMismatch {
        expected: u64,
        actual: u64,
        contract: String,
    },

    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosError(Box<cosmos::Error>),

    #[cfg(feature = "ethereum")]
    #[error("Failed to decode FundsReceived: {0}")]
    FailedToDecodeFundsReceived(#[source] alloy::sol_types::Error),

    #[cfg(feature = "ethereum")]
    #[error("Invalid Ethereum contract address: {contract}: {error}")]
    InvalidEthereumContractAddress {
        contract: String,
        #[source]
        error: const_hex::FromHexError,
    },

    #[error("Invalid Ethereum bridge contract address: {0}")]
    InvalidEthereumBridgeContractAddress(String),

    #[cfg(feature = "ethereum")]
    #[error("Invalid default Ethereum RPC URL for {chain:?}: {error}")]
    InvalidDefaultEthereumRpcUrl {
        chain: EthereumChain,
        #[source]
        error: url::ParseError,
    },

    #[cfg(feature = "ethereum")]
    #[error("Ethereum listener contract checks are not implemented yet")]
    EthereumListenerContractChecksNotImplemented,

    #[error("Ethereum payload generation is not implemented yet")]
    EthereumPayloadGenerationNotImplemented,

    #[error("Trying to configure a Cosmos contract as an Ethereum bridge.")]
    TryingToConfigureCosmosContractAsEthereumBridge,

    #[error("Trying to configure a Solana program as an Ethereum bridge.")]
    TryingToConfigureSolanaProgramAsEthereumBridge,

    #[cfg(feature = "ethereum")]
    #[error("Ethereum value {0} does not fit into u128")]
    EthereumValueDoesNotFitIntoU128(alloy::primitives::U256),

    #[cfg(feature = "ethereum")]
    #[error(transparent)]
    AlloyRpcError(
        #[from]
        alloy::providers::transport::RpcError<alloy::providers::transport::TransportErrorKind>,
    ),

    #[error("Unable to get latest archived block: {0}")]
    UnableToGetLatestArchivedBlock(#[source] KolmeStoreError),

    #[cfg(feature = "ethereum")]
    #[error("Ethereum filtered resume query returned mismatched event ID. Expected {expected}, got {actual}")]
    EthereumFilteredResumeQueryEventIdMismatch {
        actual: BridgeEventId,
        expected: BridgeEventId,
    },

    #[cfg(feature = "ethereum")]
    #[error("Unexpected Ethereum bridge event ID. Expected {expected}, got {actual}")]
    UnexpectedEthereumBridgeEventId {
        actual: BridgeEventId,
        expected: BridgeEventId,
    },
    //@@@ WHAT STILL USES THIS?
    #[error("{0}")]
    Other(String),
}

impl KolmeError {
    pub fn other<E: std::fmt::Display>(e: E) -> Self {
        Self::Other(e.to_string())
    }
}

#[cfg(feature = "solana")]
impl From<solana_client::nonblocking::pubsub_client::PubsubClientError> for KolmeError {
    fn from(e: solana_client::nonblocking::pubsub_client::PubsubClientError) -> Self {
        Self::SolanaPubsubClientError(Box::new(e))
    }
}

#[cfg(feature = "cosmwasm")]
impl From<CosmosConfigError> for KolmeError {
    fn from(e: CosmosConfigError) -> Self {
        Self::CosmosConfigError(Box::new(e))
    }
}

#[cfg(feature = "cosmwasm")]
impl From<BuilderError> for KolmeError {
    fn from(e: BuilderError) -> Self {
        Self::BuilderError(Box::new(e))
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for KolmeError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(Box::new(e))
    }
}

#[cfg(feature = "solana")]
impl From<solana_client::client_error::ClientError> for KolmeError {
    fn from(e: solana_client::client_error::ClientError) -> Self {
        Self::SolanaClient(Box::new(e))
    }
}

#[cfg(feature = "cosmwasm")]
impl From<cosmos::Error> for KolmeError {
    fn from(e: cosmos::Error) -> Self {
        Self::CosmosError(Box::new(e))
    }
}

//@@@ WHAT IS THIS FOR?
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

// @@@ MAYBE RENAME TO KolmeTransactionError AND MOVE TO TOP TO REDUCE NUMBER OF DIFFs?
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
