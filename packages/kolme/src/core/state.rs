use crate::*;

#[derive(snafu::Snafu, Debug)]
pub enum CoreStateError {
    ChainNotSupported {
        chain: ExternalChain,
    },
    AssetNotSupported {
        chain: ExternalChain,
        asset_id: AssetId,
    },
}

/// Raw framework state that can be serialized to the database.
#[derive(Clone, Debug)]
pub struct FrameworkState {
    pub(super) processor: PublicKey,
    pub(super) listeners: BTreeSet<PublicKey>,
    pub(super) needed_listeners: usize,
    pub(super) approvers: BTreeSet<PublicKey>,
    pub(super) needed_approvers: usize,
    pub(super) chains: BTreeMap<ExternalChain, ChainConfig>,
    pub(super) balances: Balances,
}

impl MerkleSerialize for FrameworkState {
    fn serialize<S: MerkleSerializer>(
        &mut self,
        serializer: &mut S,
    ) -> std::result::Result<(), MerkleSerialError> {
        let FrameworkState {
            processor,
            listeners,
            needed_listeners,
            approvers,
            needed_approvers,
            chains,
            balances,
        } = self;
        serializer.store_json(processor)?;
        serializer.store_json(listeners)?;
        serializer.store_json(needed_listeners)?;
        serializer.store_json(approvers)?;
        serializer.store_json(needed_approvers)?;
        serializer.store_json(chains)?;
        todo!()
    }
}

impl FrameworkState {
    fn new(
        GenesisInfo {
            kolme_ident: _,
            processor,
            listeners,
            needed_listeners,
            approvers,
            needed_approvers,
            chains,
        }: &GenesisInfo,
    ) -> Self {
        FrameworkState {
            processor: *processor,
            listeners: listeners.clone(),
            needed_listeners: *needed_listeners,
            approvers: approvers.clone(),
            needed_approvers: *needed_approvers,
            chains: chains.clone(),
            balances: Balances::default(),
        }
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.approvers.len() >= self.needed_approvers);
        anyhow::ensure!(self.needed_approvers > 0);
        Ok(())
    }

    pub(super) fn get_asset_config(
        &self,
        chain: ExternalChain,
        asset_id: AssetId,
    ) -> Result<&AssetConfig, CoreStateError> {
        self.chains
            .get(&chain)
            .ok_or(CoreStateError::ChainNotSupported { chain })?
            .assets
            .values()
            .find(|config| config.asset_id == asset_id)
            .ok_or(CoreStateError::AssetNotSupported { chain, asset_id })
    }
}

pub(super) struct LoadStateResult<AppState> {
    pub(super) framework_state: FrameworkState,
    pub(super) app_state: AppState,
    pub(super) next_height: BlockHeight,
    pub(super) current_block_hash: BlockHash,
}

pub(super) async fn load_state<App: KolmeApp>(
    pool: &sqlx::SqlitePool,
    genesis: &GenesisInfo,
) -> Result<LoadStateResult<App::State>> {
    struct Output {
        height: i64,
        blockhash: Vec<u8>,
        framework_state_hash: Vec<u8>,
        app_state_hash: Vec<u8>,
    }
    let output = sqlx::query_as!(
        Output,
        r#"
            SELECT height, blockhash, framework_state_hash, app_state_hash
            FROM blocks
            ORDER BY height DESC
            LIMIT 1
        "#
    )
    .fetch_optional(pool)
    .await?;
    let res = match output {
        Some(Output {
            framework_state_hash,
            app_state_hash,
            height,
            blockhash,
        }) => {
            let framework_state = load_by_raw_hash(pool, &framework_state_hash).await?;
            let framework_state = serde_json::from_str(&framework_state)?;
            let app_state = load_by_raw_hash(pool, &app_state_hash).await?;
            let app_state = App::load_state(&app_state)?;
            let height = BlockHeight::try_from(height)?;
            let next_height = height.next();
            let current_block_hash = BlockHash(Sha256Hash::from_hash(&blockhash)?);
            LoadStateResult {
                framework_state,
                app_state,
                next_height,
                current_block_hash,
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

async fn load_by_raw_hash(pool: &sqlx::SqlitePool, hash: &[u8]) -> Result<String> {
    sqlx::query_scalar!("SELECT content FROM hashes WHERE hash=$1", hash)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

/// Ensures that either we have no blocks yet, or the first block has matching genesis info.
pub(super) async fn validate_genesis_info(
    pool: &sqlx::SqlitePool,
    expected: &GenesisInfo,
) -> Result<()> {
    if let Some(actual) = load_genesis_info(pool).await? {
        anyhow::ensure!(&actual == expected);
    }
    Ok(())
}

async fn load_genesis_info(pool: &sqlx::SqlitePool) -> Result<Option<GenesisInfo>> {
    let Some(rendered) = sqlx::query_scalar!(
        r#"
            SELECT rendered
            FROM blocks
            WHERE height=0
        "#
    )
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let SignedBlock::<()>(signed) = serde_json::from_str(&rendered)?;
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
