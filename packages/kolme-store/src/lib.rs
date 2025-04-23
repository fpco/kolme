use merkle_map::{MerkleSerialError, Sha256Hash};

/// Contents of a block to be stored in a database.
pub struct StorableBlock<FrameworkState, AppState> {
    pub height: u64,
    pub blockhash: Sha256Hash,
    pub txhash: Sha256Hash,
    pub rendered: String,
    pub framework_state: FrameworkState,
    pub app_state: AppState,
    pub logs: Vec<Vec<String>>,
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeStoreError {
    #[error(transparent)]
    Custom(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Merkle(#[from] MerkleSerialError),
    #[error("Block not found in storage: {height}")]
    BlockNotFound { height: u64 },
}

impl KolmeStoreError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(Box::new(e))
    }
}
