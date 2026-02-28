use merkle_map::{MerkleSerialError, Sha256Hash};

#[derive(Debug, Clone, Copy, strum::Display)]
pub enum StorageBackend {
    Fjall,
    Postgres,
    InMemory,
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeStoreError {
    #[error(transparent)]
    Custom(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Merkle(#[from] MerkleSerialError),
    #[error("Block not found in storage: {height}")]
    BlockNotFound { height: u64 },
    #[error("KolmeStore::delete_block is not supported by this store: {0}")]
    UnsupportedDeleteOperation(StorageBackend),
    // kolme#144 - Reports a diverging hash with same height
    #[error("Block with height {height} in database with different hash {existing}, trying to add {adding}")]
    ConflictingBlockInDb {
        height: u64,
        adding: Sha256Hash,
        existing: Sha256Hash,
    },
    // kolme#144 - Reports a double insert (Block already exists with same hash and insert)
    #[error("Block already in database: {height}")]
    MatchingBlockAlreadyInserted { height: u64 },
    #[error("Transaction is already present in database: {txhash}")]
    TxAlreadyInDb { txhash: Sha256Hash },
    #[error("get_height_for_tx: invalid height in {backend} store: {reason}")]
    InvalidHeight {
        backend: StorageBackend,
        #[source]
        reason: std::array::TryFromSliceError,
    },
    #[error("Merkle validation error: child hash {child} not found")]
    MissingMerkleChild { child: Sha256Hash },
    #[error("Invalid genesis message count in first block")]
    InvalidGenesisMessageCount,
    #[error("Invalid message type in first block: expected Genesis")]
    InvalidFirstBlockMessageType,
    #[error(transparent)]
    FjallError(#[from] fjall::Error),
    #[error("Unable to update partition with given height: {0}")]
    UnableToUpdatePartition(#[source] fjall::Error),
    #[error("Unable to retrieve latest height: {0}")]
    UnableToRetrieveLatestHeight(#[source] fjall::Error),
    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),
    #[error("Could not connect to the database: {0}")]
    CouldNotConnectToDatabase(#[source] sqlx::Error),
    #[error("Unable to execute migrations: {0}")]
    UnableToExecuteMigrations(#[source] sqlx::migrate::MigrateError),
    #[error("Unable to query tx height: {0}")]
    UnableToQueryTxHeight(#[source] sqlx::Error),
    #[error(transparent)]
    TryFromIntError(#[from] std::num::TryFromIntError),
    #[error("Unable to start database: {0}")]
    UnableToStartDatabase(#[source] sqlx::Error),
    #[error("Unable to store latest archived block height: {0}")]
    UnableToStoreLatestArchivedBlockHeight(#[source] sqlx::Error),
    #[error("Unable to refresh materialized view: {0}")]
    UnableToRefreshMaterializedView(#[source] sqlx::Error),
    #[error("Unable to commit archive block height changes: {0}")]
    UnableToCommitArchiveBlockHeightChanges(#[source] sqlx::Error),
    #[error("{0}")]
    Other(String),
}

impl KolmeStoreError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(Box::new(e))
    }
}
