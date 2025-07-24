use std::sync::Arc;

use crate::{KolmeConstructLock, KolmeStoreError, StorableBlock};
use enum_dispatch::enum_dispatch;
use merkle_map::{
    MerkleContents, MerkleDeserializeRaw, MerkleLayerContents, MerkleSerialError,
    MerkleSerializeRaw, Sha256Hash,
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
        height: u64,
    ) -> Result<Option<StorableBlock<Block, FrameworkState, AppState>>, KolmeStoreError>
    where
        Block: serde::de::DeserializeOwned + MerkleDeserializeRaw + MerkleSerializeRaw,
        FrameworkState: MerkleDeserializeRaw + MerkleSerializeRaw,
        AppState: MerkleDeserializeRaw + MerkleSerializeRaw;

    async fn has_block(&self, height: u64) -> Result<bool, KolmeStoreError>;
    async fn has_merkle_hash(&self, hash: Sha256Hash) -> Result<bool, MerkleSerialError>;

    async fn add_block<Block, FrameworkState, AppState>(
        &self,
        block: &StorableBlock<Block, FrameworkState, AppState>,
    ) -> Result<(), KolmeStoreError>
    where
        Block: serde::Serialize + MerkleSerializeRaw,
        FrameworkState: MerkleSerializeRaw,
        AppState: MerkleSerializeRaw;
    async fn add_merkle_layer(
        &self,
        hash: Sha256Hash,
        layer: &MerkleLayerContents,
    ) -> anyhow::Result<()>;

    async fn archive_block(&self, height: u64) -> anyhow::Result<()>;
    async fn get_latest_archived_block_height(&self) -> anyhow::Result<Option<u64>>;

    async fn save<T>(&self, value: &T) -> anyhow::Result<Arc<MerkleContents>>
    where
        T: MerkleSerializeRaw;
    async fn load<T>(&self, hash: Sha256Hash) -> Result<T, MerkleSerialError>
    where
        T: MerkleDeserializeRaw;

    async fn get_next_missing_layer(&self) -> anyhow::Result<Option<u64>>;
    async fn set_next_missing_layer(&self, height: u64) -> anyhow::Result<()>;
}
