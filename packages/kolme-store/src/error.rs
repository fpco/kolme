use merkle_map::{MerkleSerialError, Sha256Hash};

#[derive(Debug, strum::Display)]
pub enum StorageBackend {
    Fjall,
    Postgres,
    InMemory,
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeStoreError {
    #[error(transparent)]
    Custom(Box<dyn std::error::Error + Send + Sync>),
    #[error("{context}: {error}")]
    CustomContext {
        context: String,
        #[source]
        error: Box<dyn std::error::Error + Send + Sync>,
    },
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
    #[error("Merkle validation error: child hash {child} not found")]
    MissingMerkleChild { child: Sha256Hash },
    #[error("{0}")]
    Other(String),
}

impl KolmeStoreError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(Box::new(e))
    }
    pub fn custom_context<E: std::error::Error + Send + Sync + 'static>(
        context: &str,
        e: E,
    ) -> Self {
        Self::CustomContext {
            context: context.to_owned(),
            error: Box::new(e),
        }
    }
}
