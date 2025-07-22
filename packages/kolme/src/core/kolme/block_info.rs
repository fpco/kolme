use kolme_store::StorableBlock;

use crate::core::*;

/// Information on a specific block.
pub(in crate::core) struct BlockInfo<App: KolmeApp> {
    pub(super) block: Arc<SignedBlock<App::Message>>,
    #[allow(dead_code)]
    pub(super) logs: Arc<[Vec<String>]>,
    pub(super) state: BlockState<App>,
}

/// Separated from [BlockInfo] since it can also represent initial state before any blocks.
pub(in crate::core) struct BlockState<App: KolmeApp> {
    pub(super) blockhash: BlockHash,
    pub(super) framework_state: Arc<FrameworkState>,
    pub(super) app_state: Arc<App::State>,
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

    pub(super) async fn load(store: &KolmeStore<App>, app: &App) -> Result<Self> {
        let output = store.load_latest_block().await?;
        let res = match output {
            Some(height) => {
                let storable = store.load_block(height).await?.with_context(|| {
                    format!(
                        "Latest block height is {height}, but it wasn't found in the data store"
                    )
                })?;
                MaybeBlockInfo::Some(storable.try_into()?)
            }
            None => MaybeBlockInfo::None(BlockState {
                framework_state: Arc::new(FrameworkState::new(app.genesis_info())),
                app_state: Arc::new(app.new_state()?),
                blockhash: BlockHash::genesis_parent(),
            }),
        };
        res.get_framework_state().validate()?;
        Ok(res)
    }
}

impl<App: KolmeApp> TryFrom<StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>>
    for BlockInfo<App>
{
    type Error = anyhow::Error;

    fn try_from(
        StorableBlock {
            height,
            blockhash,
            txhash: _,
            block,
            framework_state,
            app_state,
            logs,
        }: StorableBlock<SignedBlock<App::Message>, FrameworkState, App::State>,
    ) -> Result<Self> {
        anyhow::ensure!(height == block.height().0);
        Ok(Self {
            block,
            logs,
            state: BlockState {
                blockhash: BlockHash(blockhash),
                framework_state,
                app_state,
            },
        })
    }
}
