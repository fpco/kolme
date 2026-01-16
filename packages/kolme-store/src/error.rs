use merkle_map::{MerkleSerialError, Sha256Hash};

#[derive(Debug, Clone, Copy)]
pub enum StorageBackend {
    Fjall,
    Postgres,
    InMemory,
}

impl std::fmt::Display for StorageBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageBackend::Fjall => write!(f, "Fjall"),
            StorageBackend::Postgres => write!(f, "Postgres"),
            StorageBackend::InMemory => write!(f, "InMemory"),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeStoreError {
    #[error("Custom error: {0}")]
    Custom(String),

    #[error(transparent)]
    Merkle(#[from] MerkleSerialError),

    #[error("Block not found in storage: {height}")]
    BlockNotFound { height: u64 },

    #[error("KolmeStore::delete_block is not supported by this store: {backend}")]
    UnsupportedDeleteOperation { backend: StorageBackend },

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

    #[error("get_height_for_tx: invalid height bytes in {backend} store for tx {txhash:?}, bytes: {bytes:?}, reason: {reason}")]
    InvalidHeight {
        backend: StorageBackend,
        txhash: Sha256Hash,
        bytes: Vec<u8>,
        reason: String,
    },

    #[error("Merkle validation error: child hash {child} not found")]
    MissingMerkleChild { child: Sha256Hash },

    #[error("Invalid genesis message count in first block")]
    InvalidGenesisMessageCount,

    #[error("Invalid message type in first block: expected Genesis")]
    InvalidFirstBlockMessageType,
}
impl KolmeStoreError {
    pub fn custom<E: std::fmt::Display>(e: E) -> Self {
        Self::Custom(e.to_string())
    }
}
