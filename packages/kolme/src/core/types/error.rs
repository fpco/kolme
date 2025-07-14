use crate::core::*;

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

    #[error("Unsupported wallet address: {address}")]
    UnsupportedWalletAddress { address: String },

    #[error("Invalid category name {name}")]
    InvalidCategoryName { name: String },

    #[error("Error broadcasting:\n{error}\n{trace}")]
    BroadcastError { error: String, trace: String },

    #[error("Already have a bridge contract for {chain:?}, just received another from a listener")]
    BridgeAlreadyDeployed { chain: ExternalChain },

    #[error(
        "Signing public key {signer} is not a member of the {role} set and cannot self-replace"
    )]
    NotInValidatorSet { signer: PublicKey, role: String },

    #[error("Signing public key {signer} is not the current processor and cannot self-replace")]
    NotProcessor { signer: PublicKey },

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
        expected_processor: PublicKey,
        actual_processor: PublicKey,
    },

    #[error("Unable to migrate contract for chain {chain:?}: contract isn't deployed")]
    ContractNotDeployed { chain: ExternalChain },

    #[error("Already have a deployed contract on {chain:?}")]
    ContractAlreadyDeployed { chain: ExternalChain },

    #[cfg(feature = "pass_through")]
    #[error("No wait for pass-through contract is expected")]
    UnexpectedPassThroughContract,

    #[error("Error with transaction {txhash} for block {proposed_height}: {error}")]
    TransactionFailed {
        txhash: TxHash,
        proposed_height: BlockHeight,
        error: String,
    },

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

    #[error("Invalid message type in the first block; expected `Genesis`")]
    InvalidGenesisMessageType,

    #[error("Listener panicked: {details}")]
    ListenerPanicked { details: String },

    #[error("Failed to execute signed Cosmos bridge transaction: {details}")]
    CosmosExecutionFailed { details: String },

    #[error("Error serializing Solana bridge payload (length): {details}")]
    SolanaPayloadLengthSerializationError { details: String },

    #[error("Error serializing Solana bridge payload (content): {details}")]
    SolanaPayloadContentSerializationError { details: String },

    #[error("Failed to calculate Borsh object length for Solana payload: {details}")]
    SolanaPayloadLengthError { details: String },

    #[error("Failed to serialize Solana payload to Borsh: {details}")]
    SolanaPayloadSerializationError { details: String },

    #[error("Failed to build Solana initialization transaction: {details}")]
    SolanaInitTxBuildFailed { details: String },

    #[error("Failed to build signed Solana transaction: {details}")]
    SolanaSignedTxBuildFailed { details: String },

    #[error("Solana submitter failed to execute signed transaction: {details}")]
    SolanaSignedTxExecutionFailed { details: String },

    #[error("Bridge program {program} hasn't been initialized yet")]
    UninitializedSolanaBridge { program: String },

    #[error("Error deserializing Solana bridge state: {details}")]
    InvalidSolanaBridgeState { details: String },

    #[error("Error deserializing Solana bridge message from logs: {details}")]
    InvalidSolanaBridgeLogMessage { details: String },

    #[error("Error deserializing Solana bridge payload: {details}")]
    InvalidSolanaPayloadDeserialization { details: String },

    #[error("{0}")]
    Other(String),
}
