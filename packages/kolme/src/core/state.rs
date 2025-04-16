use crate::*;

#[derive(thiserror::Error, Debug)]
pub enum CoreStateError {
    #[error("The chain '{chain}' is not supported")]
    ChainNotSupported { chain: ExternalChain },
    #[error("The asset '{asset_id}' on chain '{chain}' is not supported")]
    AssetNotSupported {
        chain: ExternalChain,
        asset_id: AssetId,
    },
}

/// Raw framework state that can be serialized to the database.
#[derive(Clone, Debug)]
pub struct FrameworkState {
    pub(super) processor: PublicKey,
    // FIXME replace BTreeSet with MerkleSet, and implement a MerkleSet datatype.
    pub(super) listeners: BTreeSet<PublicKey>,
    pub(super) needed_listeners: usize,
    pub(super) approvers: BTreeSet<PublicKey>,
    pub(super) needed_approvers: usize,
    pub(super) chains: BTreeMap<ExternalChain, ChainConfig>,
    pub(super) balances: Balances,
}

impl MerkleSerialize for FrameworkState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let FrameworkState {
            processor,
            listeners,
            needed_listeners,
            approvers,
            needed_approvers,
            chains,
            balances,
        } = self;
        serializer.store(processor)?;
        serializer.store(listeners)?;
        serializer.store(needed_listeners)?;
        serializer.store(approvers)?;
        serializer.store(needed_approvers)?;
        serializer.store(chains)?;
        serializer.store(balances)?;
        Ok(())
    }
}

impl MerkleDeserialize for FrameworkState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(FrameworkState {
            processor: deserializer.load()?,
            listeners: deserializer.load()?,
            needed_listeners: deserializer.load()?,
            approvers: deserializer.load()?,
            needed_approvers: deserializer.load()?,
            chains: deserializer.load()?,
            balances: deserializer.load()?,
        })
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
    merkle_manager: &MerkleManager,
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
            let framework_state_hash =
                Sha256Hash::from_array(framework_state_hash.as_slice().try_into()?);
            let framework_state = merkle_manager
                .load(&mut MerkleDbStore::Pool(pool), framework_state_hash)
                .await?;
            let app_state_hash = Sha256Hash::from_array(app_state_hash.as_slice().try_into()?);
            let app_state = merkle_manager
                .load(&mut MerkleDbStore::Pool(pool), app_state_hash)
                .await?;
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
