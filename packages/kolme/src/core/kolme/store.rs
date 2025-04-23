use crate::core::*;

use kolme_store::{KolmeStoreError, StorableBlock};
use kolme_store_sqlite::KolmeStoreSqlite;

pub(in crate::core) enum KolmeStore {
    Sqlite(KolmeStoreSqlite),
}

impl KolmeStore {
    /// Ensures that either we have no blocks yet, or the first block has matching genesis info.
    pub(super) async fn validate_genesis_info<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
        expected: &GenesisInfo,
    ) -> Result<()> {
        if let Some(actual) = self.load_genesis_info::<AppState>(merkle_manager).await? {
            anyhow::ensure!(
                &actual == expected,
                "Mismatched genesis info.\nActual:   {actual:?}\nExpected: {expected:?}"
            );
        }
        Ok(())
    }

    async fn load_genesis_info<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<GenesisInfo>> {
        let Some(block) = self
            .load_block::<AppState>(merkle_manager, BlockHeight::start())
            .await?
        else {
            return Ok(None);
        };
        let SignedBlock::<()>(signed) = serde_json::from_str(&block.rendered)?;
        let mut messages = signed
            .message
            .into_inner()
            .tx
            .0
            .message
            .into_inner()
            .messages;
        anyhow::ensure!(messages.len() == 1);
        match messages.remove(0) {
            Message::Genesis(genesis_info) => Ok(Some(genesis_info)),
            _ => Err(anyhow::anyhow!("Invalid messages in first block")),
        }
    }
}

pub(super) struct LoadStateResult<AppState> {
    pub(super) framework_state: FrameworkState,
    pub(super) app_state: AppState,
    pub(super) next_height: BlockHeight,
    pub(super) current_block_hash: BlockHash,
}

impl KolmeStore {
    pub async fn load_latest_block<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
    ) -> Result<Option<StorableBlock<FrameworkState, AppState>>> {
        Ok(match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => {
                kolme_store_sqlite.load_latest_block(merkle_manager).await?
            }
        })
    }

    pub async fn load_block<AppState: MerkleDeserialize>(
        &self,
        merkle_manager: &MerkleManager,
        height: BlockHeight,
    ) -> Result<Option<StorableBlock<FrameworkState, AppState>>> {
        let res = match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => {
                kolme_store_sqlite
                    .load_block(merkle_manager, height.0)
                    .await
            }
        };
        match res {
            Err(KolmeStoreError::BlockNotFound { height: _ }) => Ok(None),
            Err(e) => Err(e.into()),
            Ok(block) => Ok(Some(block)),
        }
    }

    pub(super) async fn load_state<App: KolmeApp>(
        &self,
        genesis: &GenesisInfo,
        merkle_manager: &MerkleManager,
    ) -> Result<LoadStateResult<App::State>> {
        let output = self.load_latest_block(merkle_manager).await?;
        let res = match output {
            Some(StorableBlock {
                height,
                blockhash,
                txhash: _,
                rendered: _,
                framework_state,
                app_state,
                logs: _,
            }) => {
                let height = BlockHeight(height);
                let next_height = height.next();
                LoadStateResult {
                    framework_state,
                    app_state,
                    next_height,
                    current_block_hash: BlockHash(blockhash),
                }
            }
            None => LoadStateResult {
                framework_state: FrameworkState::new(genesis),
                app_state: App::new_state()?,
                next_height: BlockHeight::start(),
                current_block_hash: BlockHash::genesis_parent(),
            },
        };
        res.framework_state.validate()?;
        Ok(res)
    }

    pub(super) async fn get_height_for_tx(&self, txhash: TxHash) -> Result<Option<BlockHeight>> {
        match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => kolme_store_sqlite
                .get_height_for_tx(txhash.0)
                .await
                .map(|x| x.map(BlockHeight))
                .map_err(anyhow::Error::from),
        }
    }

    pub(super) async fn add_block<AppState: MerkleSerialize>(
        &self,
        merkle_manager: &MerkleManager,
        block: &StorableBlock<FrameworkState, AppState>,
    ) -> Result<()> {
        match self {
            KolmeStore::Sqlite(kolme_store_sqlite) => kolme_store_sqlite
                .add_block(merkle_manager, block)
                .await
                .map_err(anyhow::Error::from),
        }
    }
}
