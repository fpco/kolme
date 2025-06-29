use merkle_map::{MerkleDeserialize, MerkleSerialError, MerkleSerialize, Sha256Hash};
use std::sync::Arc;

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
