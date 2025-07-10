use std::sync::Arc;

use crate::{KolmeConstructLock, KolmeStoreError, StorableBlock};
use enum_dispatch::enum_dispatch;
use merkle_map::{
    MerkleContents, MerkleDeserialize, MerkleDeserializeRaw, MerkleLayerContents, MerkleManager,
    MerkleSerialError, MerkleSerialize, MerkleSerializeRaw, Sha256Hash,
};

#[enum_dispatch(KolmeStore)]
#[allow(async_fn_in_trait)]
pub trait KolmeBackingStore {
    async fn clear_blocks(&self) -> Result<(), KolmeStoreError>;
    async fn delete_block(&self, height: u64) -> Result<(), KolmeStoreError>;

    async fn take_construct_lock(&self) -> Result<KolmeConstructLock, KolmeStoreError>;

    async fn get_merkle_layer(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleLayerContents>, MerkleSerialError>;
    async fn get_height_for_tx(&self, txhash: Sha256Hash) -> anyhow::Result<Option<u64>>;

    async fn load_latest_block(&self) -> Result<Option<u64>, KolmeStoreError>;
    async fn load_block<Block, FrameworkState, AppState>(
        &self,
        merkle_manager: &MerkleManager,
        height: u64,
    ) -> Result<Option<StorableBlock<Block, FrameworkState, AppState>>, KolmeStoreError>
    where
        Block: serde::de::DeserializeOwned + MerkleDeserialize + MerkleSerialize,
        FrameworkState: MerkleDeserialize + MerkleSerialize,
        AppState: MerkleDeserialize + MerkleSerialize;

    async fn has_block(&self, height: u64) -> Result<bool, KolmeStoreError>;
    async fn has_merkle_hash(&self, hash: Sha256Hash) -> Result<bool, MerkleSerialError>;

    async fn add_block<Block, FrameworkState, AppState>(
        &self,
        merkle_manager: &MerkleManager,
        block: &StorableBlock<Block, FrameworkState, AppState>,
    ) -> Result<(), KolmeStoreError>
    where
        Block: serde::Serialize + MerkleSerialize,
        FrameworkState: MerkleSerialize,
        AppState: MerkleSerialize;
    async fn add_merkle_layer(
        &self,
        hash: Sha256Hash,
        layer: &MerkleLayerContents,
    ) -> anyhow::Result<()>;

    async fn archive_block(&self, height: u64) -> anyhow::Result<()>;
    async fn get_latest_archived_block_height(&self) -> anyhow::Result<Option<u64>>;

    async fn save<T>(
        &self,
        merkle_manager: &MerkleManager,
        value: &T,
    ) -> anyhow::Result<Arc<MerkleContents>>
    where
        T: MerkleSerializeRaw;
    async fn load<T>(
        &self,
        merkle_manager: &MerkleManager,
        hash: Sha256Hash,
    ) -> Result<T, MerkleSerialError>
    where
        T: MerkleDeserializeRaw;
}
