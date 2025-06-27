use std::sync::Arc;
mod block;
mod error;
mod fjall;
mod postgres;
mod r#trait;

use merkle_map::{MerkleDeserialize, MerkleSerialError, MerkleSerialize, Sha256Hash};

/// Contents of a block to be stored in a database.
#[derive(Debug)]
pub struct StorableBlock<Block, FrameworkState, AppState> {
    pub height: u64,
    pub blockhash: Sha256Hash,
    pub txhash: Sha256Hash,
    pub block: Arc<Block>,
    pub framework_state: Arc<FrameworkState>,
    pub app_state: Arc<AppState>,
    pub logs: Arc<[Vec<String>]>,
pub enum KolmeConstructLock {
    NoLocking,
    Postgres { _lock: postgres::ConstructLock },
    InProcess { _lock: OwnedSemaphorePermit },
}

impl<Block, FrameworkState, AppState> Clone for StorableBlock<Block, FrameworkState, AppState> {
    fn clone(&self) -> Self {
        Self {
            height: self.height,
            blockhash: self.blockhash,
            txhash: self.txhash,
            block: self.block.clone(),
            framework_state: self.framework_state.clone(),
            app_state: self.app_state.clone(),
            logs: self.logs.clone(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeStoreError {
    #[error(transparent)]
    Custom(Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    Merkle(#[from] MerkleSerialError),
    #[error("Block not found in storage: {height}")]
    BlockNotFound { height: u64 },
    // kolme#144 - Reports a diverging hash with same height
    #[error("Block with height {height} in database with different hash {hash}")]
    ConflictingBlockInDb { height: u64, hash: Sha256Hash },
    // kolme#144 - Reports a double insert (Block already exists with same hash and insert)
    #[error("Block already in database: {height}")]
    MatchingBlockAlreadyInserted { height: u64 },
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

impl<Block: MerkleSerialize, FrameworkState: MerkleSerialize, AppState: MerkleSerialize>
    MerkleSerialize for StorableBlock<Block, FrameworkState, AppState>
{
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let Self {
            height,
            blockhash,
            txhash,
            block,
            framework_state,
            app_state,
            logs,
        } = self;
        serializer.store(height)?;
        serializer.store(blockhash)?;
        serializer.store(txhash)?;
        serializer.store(block)?;
        serializer.store(framework_state)?;
        serializer.store(app_state)?;
        serializer.store(logs)?;
        Ok(())
    }
}

impl<
        Block: MerkleSerialize + MerkleDeserialize,
        FrameworkState: MerkleSerialize + MerkleDeserialize,
        AppState: MerkleSerialize + MerkleDeserialize,
    > MerkleDeserialize for StorableBlock<Block, FrameworkState, AppState>
{
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let height = deserializer.load()?;
        let blockhash = deserializer.load()?;
        let txhash = deserializer.load()?;
        let block = deserializer.load()?;
        let framework_state = deserializer.load()?;
        let app_state = deserializer.load()?;
        let logs = deserializer.load().map(|x: Vec<Vec<String>>| x.into())?;
        Ok(Self {
            height,
            blockhash,
            txhash,
            block,
            framework_state,
            app_state,
            logs,
        })
    }
}
