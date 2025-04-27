use kolme_store::StorableBlock;

use crate::core::*;

/// Information on a specific block.
pub(in crate::core) struct BlockInfo<App: KolmeApp> {
    pub(super) block: Arc<SignedBlock<App::Message>>,
    #[allow(dead_code)]
    rendered: Arc<str>,
    #[allow(dead_code)]
    logs: Arc<[Vec<String>]>,
    state: BlockState<App>,
}

/// Separated from [BlockInfo] since it can also represent initial state before any blocks.
pub(in crate::core) struct BlockState<App: KolmeApp> {
    blockhash: BlockHash,
    framework_state: Arc<FrameworkState>,
    app_state: Arc<App::State>,
}

/// Either the info on a block or initial pre-genesis state.
pub(in crate::core) enum MaybeBlockInfo<App: KolmeApp> {
    None(BlockState<App>),
    Some(BlockInfo<App>),
}

impl<App: KolmeApp> MaybeBlockInfo<App> {
    fn get_state(&self) -> &BlockState<App> {
        match self {
            MaybeBlockInfo::None(block_state) => block_state,
            MaybeBlockInfo::Some(block_info) => &block_info.state,
        }
    }

    pub(super) fn get_app_state(&self) -> &App::State {
        &self.get_state().app_state
    }

    pub(super) fn get_framework_state(&self) -> &FrameworkState {
        &self.get_state().framework_state
    }

    pub(super) fn get_block_hash(&self) -> BlockHash {
        self.get_state().blockhash
    }

    pub(super) fn get_next_height(&self) -> BlockHeight {
        match self {
            MaybeBlockInfo::None(_) => BlockHeight::start(),
            MaybeBlockInfo::Some(block_info) => block_info.block.height().next(),
        }
    }

    pub(super) async fn load(
        store: &KolmeStore<App>,
        genesis: &GenesisInfo,
        merkle_manager: &MerkleManager,
    ) -> Result<Self> {
        let output = store.load_latest_block().await?;
        let res = match output {
            Some(height) => {
                let storable = store.load_block(merkle_manager, height).await?.with_context(|| format!("Latest block height is {height}, but it wasn't found in the data store"))?;
                MaybeBlockInfo::Some(storable.try_into()?)
            }
            None => MaybeBlockInfo::None(BlockState {
                framework_state: Arc::new(FrameworkState::new(genesis)),
                app_state: Arc::new(App::new_state()?),
                blockhash: BlockHash::genesis_parent(),
            }),
        };
        res.get_framework_state().validate()?;
        Ok(res)
    }
}

impl<App: KolmeApp> BlockInfo<App> {
    pub(super) fn get_height(&self) -> BlockHeight {
        self.block.height()
    }
}

impl<App: KolmeApp> TryFrom<StorableBlock<FrameworkState, App::State>> for BlockInfo<App> {
    type Error = anyhow::Error;

    fn try_from(
        StorableBlock {
            height,
            blockhash,
            txhash: _,
            rendered,
            framework_state,
            app_state,
            logs,
        }: StorableBlock<FrameworkState, App::State>,
    ) -> Result<Self> {
        let block = serde_json::from_str::<SignedBlock<App::Message>>(&rendered)?;
        anyhow::ensure!(height == block.height().0);
        Ok(Self {
            block: Arc::new(block),
            rendered,
            logs,
            state: BlockState {
                blockhash: BlockHash(blockhash),
                framework_state,
                app_state,
            },
        })
    }
}
