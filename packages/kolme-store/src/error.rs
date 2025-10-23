use merkle_map::{MerkleSerialError, Sha256Hash};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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

#[derive(thiserror::Error, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum KolmeStoreError {
    #[error("Custom error: {0}")]
    Custom(String),

    #[error("Merkle error: {0}")]
    Merkle(String),

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

    #[error("get_height_for_tx: invalid height bytes in Fjall store for tx {txhash:?}, bytes: {bytes:?}, reason: {reason}")]
    InvalidHeightInFjall {
        txhash: Sha256Hash,
        bytes: Vec<u8>,
        reason: String,
    },

    #[error("{0}")]
    Other(String),
}

impl KolmeStoreError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(e.to_string())
    }
}

impl From<fjall::Error> for KolmeStoreError {
    fn from(e: fjall::Error) -> Self {
        KolmeStoreError::Other(format!("Fjall error: {e}"))
    }
}

impl From<anyhow::Error> for KolmeStoreError {
    fn from(e: anyhow::Error) -> Self {
        KolmeStoreError::Other(format!("Anyhow Error: {e}"))
    }
}

impl From<MerkleSerialError> for KolmeStoreError {
    fn from(e: MerkleSerialError) -> Self {
        KolmeStoreError::Other(format!("Merkle Serial Error: {e}"))
    }
}
