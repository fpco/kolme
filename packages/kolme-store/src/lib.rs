use merkle_map::{MerkleDeserialize, MerkleSerialError, MerkleSerialize, Sha256Hash};

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
    #[error("Block already in database: {height}")]
    BlockAlreadyInDb { height: u64 },
    #[error("Transaction is already present in database: {txhash}")]
    TxAlreadyInDb { txhash: Sha256Hash },
    #[error("{0}")]
    Other(String),
}

impl KolmeStoreError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(Box::new(e))
    }
}

impl<FrameworkState: MerkleSerialize, AppState: MerkleSerialize> MerkleSerialize
    for StorableBlock<FrameworkState, AppState>
{
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let Self {
            height,
            blockhash,
            txhash,
            rendered,
            framework_state,
            app_state,
            logs,
        } = self;
        serializer.store(height)?;
        serializer.store(blockhash)?;
        serializer.store(txhash)?;
        serializer.store(rendered)?;
        serializer.store(framework_state)?;
        serializer.store(app_state)?;
        serializer.store(logs)?;
        Ok(())
    }
}

impl<FrameworkState: MerkleDeserialize, AppState: MerkleDeserialize> MerkleDeserialize
    for StorableBlock<FrameworkState, AppState>
{
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            height: deserializer.load()?,
            blockhash: deserializer.load()?,
            txhash: deserializer.load()?,
            rendered: deserializer.load()?,
            framework_state: deserializer.load()?,
            app_state: deserializer.load()?,
            logs: deserializer.load()?,
        })
    }
}
