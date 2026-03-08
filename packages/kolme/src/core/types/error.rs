use crate::core::*;

#[derive(thiserror::Error, Debug, Clone, serde::Serialize, serde::Deserialize)]
/// Errors that need to be serialized on order to report through Gossip.
pub enum KolmeTransactionError {
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
    #[error("Start height {start} is greater than {end} height")]
    InvalidBlockHeight {
        start: BlockHeight,
        end: BlockHeight,
    },
    #[error("{0}")]
    Other(String),
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeError {
    #[error("Already have a bridge contract for {0:?}, just received another from a listener")]
    BridgeAlreadyDeployed(ExternalChain),
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
        "Signing public key {signer} is not a member of the {} set and cannot self-replace",
        if *is_approver { "approver" } else { "listener" }
    )]
    SigningKeyNotInValidatorSet {
        signer: Box<PublicKey>,
        is_approver: bool,
    },
    #[error("Signing public key {0} is not the current processor and cannot self-replace")]
    NotCurrentProcessor(Box<PublicKey>),
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
    #[error("Tried to add block with height {0}, but it's already present in the store")]
    BlockAlreadyExists(BlockHeight),
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
    #[error("Unable to migrate contract for chain {0:?}: contract isn't deployed")]
    ContractNotDeployed(ExternalChain),
    #[error("Already have a deployed contract on {0:?}")]
    ContractAlreadyDeployed(ExternalChain),
    #[cfg(feature = "pass_through")]
    #[error("No wait for pass-through contract is expected")]
    UnexpectedPassThroughContract,
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
    #[error("Listener panicked: {0}")]
    ListenerPanicked(#[source] tokio::task::JoinError),
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
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error("Mismatched genesis info: actual {actual:?}, expected {expected:?}")]
    MismatchedGenesisInfo {
        actual: Box<GenesisInfo>,
        expected: Box<GenesisInfo>,
    },
    #[error("Identical proposal {0} already exists")]
    DuplicateAdminProposal(AdminProposalId),
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
    #[error(transparent)]
    ImportExportError(#[from] ImportExportError),
    #[error(transparent)]
    StoreError(#[from] kolme_store::KolmeStoreError),
    #[error(transparent)]
    SecretKeyError(#[from] SecretKeyError),
    #[error("Transaction already in mempool")]
    TxAlreadyInMempool,
    #[error("Transaction already included in block {0}")]
    TxAlreadyInBlock(BlockHeight),
    #[error("Error serializing Solana bridge payload: {0}")]
    SolanaPayloadSerializationError(#[source] std::io::Error),
    #[cfg(feature = "solana")]
    #[error(transparent)]
    SolanaPubsubClientError(Box<solana_client::nonblocking::pubsub_client::PubsubClientError>),
    #[error("Bridge program {0} hasn't been initialized yet")]
    UninitializedSolanaBridge(String),
    #[error("Error deserializing Solana bridge state: {0}")]
    InvalidSolanaBridgeState(#[source] std::io::Error),
    #[error("Error deserializing Solana bridge message from logs: {0}")]
    InvalidSolanaBridgeLogMessage(#[source] std::io::Error),
    #[error(transparent)]
    TransactionError(KolmeTransactionError),
    #[error(transparent)]
    BroadcastRecvError(#[from] tokio::sync::broadcast::error::RecvError),
    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosAddressError(#[from] cosmos::error::AddressError),
    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosWalletError(#[from] cosmos::error::WalletError),
    #[cfg(feature = "solana")]
    #[error(transparent)]
    ParseSolanaPubkeyError(#[from] kolme_solana_bridge_client::pubkey::ParsePubkeyError),
    #[error(transparent)]
    CoreStateError(#[from] CoreStateError),
    #[error(transparent)]
    AssetError(#[from] AssetError),
    #[error(transparent)]
    AccountsError(#[from] AccountsError),
    #[error(transparent)]
    ValidatorSetError(#[from] ValidatorSetError),
    #[error(transparent)]
    PublicKeyError(#[from] PublicKeyError),
    #[error(transparent)]
    MerkleSerialError(#[from] MerkleSerialError),
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    WatchRecvError(#[from] tokio::sync::watch::error::RecvError),
    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosConfigError(Box<cosmos::CosmosConfigError>),
    #[cfg(feature = "cosmwasm")]
    #[error(transparent)]
    CosmosBuilderError(Box<cosmos::error::BuilderError>),
    #[error(transparent)]
    AxumError(#[from] axum::Error),
    #[cfg(feature = "solana")]
    #[error(transparent)]
    ParseSolanaSignatureError(#[from] solana_signature::ParseSignatureError),
    #[error(transparent)]
    TryFromIntError(#[from] std::num::TryFromIntError),
    #[error(transparent)]
    Base64DecodeError(#[from] base64::DecodeError),
    #[error("WebSocket stream terminated")]
    WebSocketClosed,
    #[error(transparent)]
    TungsteniteError(Box<tokio_tungstenite::tungstenite::Error>),
    #[cfg(feature = "solana")]
    #[error(transparent)]
    SolanaClientError(Box<solana_client::client_error::ClientError>),
    #[error("Latest block height is {0}, but it wasn't found in the data store")]
    BlockMissingInStore(BlockHeight),
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
        error: tokio::time::error::Elapsed,
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
    #[error("Invalid chain version from response: {0}")]
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
    #[error("Missing merkle layer {0} in source store")]
    MissingMerkleLayer(Sha256Hash),
    #[error("App state {0} not written to Merkle store")]
    AppStateNotWritten(Sha256Hash),
    #[error("Unable to mark block {height} as archived: {error}")]
    ArchiveBlockFailed {
        height: BlockHeight,
        #[source]
        error: kolme_store::KolmeStoreError,
    },
    #[error("Framework state {0} not written to Merkle store")]
    FrameworkStateNotWritten(Sha256Hash),
    #[error("Logs {0} not written to Merkle store")]
    MissingLogMerkleLayer(Sha256Hash),
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
    #[error("Cannot approve bridge action with a non-approver pubkey: {0}")]
    NonApproverSignature(Box<PublicKey>),
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
    #[error("Approver signature invalid: signer {0}")]
    InvalidApproverSignature(Box<PublicKey>),
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
    DataRequestError(#[from] KolmeDataRequestError),
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
    #[error("Specified an unknown proposal ID {0}")]
    UnknownAdminProposalId(AdminProposalId),
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
    UnableToGetLatestArchivedBlock(#[source] kolme_store::KolmeStoreError),
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
    #[cfg(feature = "pass_through")]
    #[error("Pass-through submission attempted on wrong chain: expected PassThrough, got {0}")]
    InvalidPassThroughChain(ExternalChain),
    #[error("Merkle validation error: child hash {0} not found")]
    MissingMerkleChild(Sha256Hash),
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
impl From<cosmos::CosmosConfigError> for KolmeError {
    fn from(e: cosmos::CosmosConfigError) -> Self {
        Self::CosmosConfigError(Box::new(e))
    }
}

#[cfg(feature = "cosmwasm")]
impl From<cosmos::error::BuilderError> for KolmeError {
    fn from(e: cosmos::error::BuilderError) -> Self {
        Self::CosmosBuilderError(Box::new(e))
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for KolmeError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::TungsteniteError(Box::new(e))
    }
}

#[cfg(feature = "solana")]
impl From<solana_client::client_error::ClientError> for KolmeError {
    fn from(e: solana_client::client_error::ClientError) -> Self {
        Self::SolanaClientError(Box::new(e))
    }
}

#[cfg(feature = "cosmwasm")]
impl From<cosmos::Error> for KolmeError {
    fn from(e: cosmos::Error) -> Self {
        Self::CosmosError(Box::new(e))
    }
}

impl<T> From<ProposeTransactionError<T>> for KolmeError {
    fn from(e: ProposeTransactionError<T>) -> Self {
        match e {
            ProposeTransactionError::InMempool => KolmeError::TxAlreadyInMempool,
            ProposeTransactionError::InBlock(block) => KolmeError::TxAlreadyInBlock(block.height()),
            ProposeTransactionError::Failed(failed) => {
                KolmeError::TransactionError(failed.message.as_inner().error.clone())
            }
        }
    }
}
