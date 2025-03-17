use crate::core::*;

pub(super) struct ExecutionStreamState<App: KolmeApp> {
    pub(super) height: EventHeight,
    pub(super) framework: TaggedJson<RawExecutionState>,
    pub(super) app: TaggedJson<App::State>,
    pub(super) genesis_info: GenesisInfo,
}

impl<App: KolmeApp> ExecutionStreamState<App> {
    pub(super) async fn load(pool: &sqlx::SqlitePool) -> Result<Option<ExecutionStreamState<App>>> {
        #[derive(serde::Deserialize)]
        struct Helper {
            height: i64,
            framework_state: Vec<u8>,
            app_state: Vec<u8>,
        }
        match sqlx::query_as!(
        Helper,
        "SELECT height, framework_state, app_state FROM execution_stream ORDER BY height DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await? {
        None => Ok(None),
        Some(Helper { height, framework_state, app_state  }) => {
            let framework_state=Sha256Hash::from_hash(&framework_state)?;
            let app_state=Sha256Hash::from_hash(&app_state)?;
            let height = height.try_into()?;
            let framework=get_state_payload(pool, &framework_state).await?;
            let framework=TaggedJson::try_from_string(framework)?;
            let app = get_state_payload(pool, &app_state).await?;
            let app = TaggedJson::from_pair(App::load_state(&app)?,app);
            let genesis_info = load_genesis_info(pool).await?;
            Ok(Some(ExecutionStreamState { height, framework, app, genesis_info }))
        },
    }
    }
}

async fn load_genesis_info(pool: &sqlx::SqlitePool) -> Result<GenesisInfo> {
    let s = sqlx::query_scalar!(
        r#"
        SELECT rendered
        FROM combined_stream
        WHERE NOT is_execution AND height=0
    "#
    )
    .fetch_one(pool)
    .await?;
    let SignedEvent::<()>(signed) = serde_json::from_str(&s)?;
    let mut messages = signed
        .message
        .into_inner()
        .event
        .0
        .message
        .into_inner()
        .messages;
    anyhow::ensure!(messages.len() == 1);
    match messages.remove(0) {
        EventMessage::Genesis(genesis_info) => Ok(genesis_info),
        _ => Err(anyhow::anyhow!("Invalid messages in first event")),
    }
}

/// Raw framework state that can be serialized to the database.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct RawExecutionState {
    code_version: String,
    processor: PublicKey,
    listeners: BTreeSet<PublicKey>,
    needed_listeners: usize,
    executors: BTreeSet<PublicKey>,
    needed_executors: usize,
    chains: BTreeMap<ExternalChain, ChainConfig>,
    // TODO: add in balances, something like this: pub balances: BTreeMap<AccountId, BTreeMap<AssetId, Decimal>>
}

impl RawExecutionState {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.executors.len() >= self.needed_executors);
        anyhow::ensure!(self.needed_executors > 0);
        Ok(())
    }
}

pub struct ExecutionState<App: KolmeApp> {
    exec: RawExecutionState,
    app: App::State,
    /// Serialized format stored to quickly reload if we need to roll back event processing.
    exec_serialized: TaggedJson<RawExecutionState>,
    app_serialized: TaggedJson<App::State>,
    next_height: EventHeight,
}

impl<App: KolmeApp> ExecutionState<App> {
    pub(super) fn new(
        code_version: impl Into<String>,
        GenesisInfo {
            kolme_ident: _,
            processor,
            listeners,
            needed_listeners,
            executors,
            needed_executors,
            chains,
        }: GenesisInfo,
    ) -> Result<Self> {
        let exec = RawExecutionState {
            code_version: code_version.into(),
            processor,
            listeners,
            needed_listeners,
            executors,
            needed_executors,
            chains,
        };
        exec.validate()?;
        let app = App::new_state()?;
        let app_serialized = App::save_state(&app)?;
        let app_serialized =
            TaggedJson::from_pair(App::load_state(&app_serialized)?, app_serialized);
        Ok(ExecutionState {
            next_height: EventHeight::start(),
            exec: exec.clone(),
            app,
            exec_serialized: TaggedJson::new(exec)?,
            app_serialized,
        })
    }

    pub(super) fn load(
        _app: &App,
        ExecutionStreamState {
            height,
            framework: exec_serialized,
            app: app_serialized,
            genesis_info,
        }: ExecutionStreamState<App>,
        code_version: &str,
        expected_genesis_info: &GenesisInfo,
    ) -> Result<Self> {
        anyhow::ensure!(&genesis_info == expected_genesis_info);
        let exec = exec_serialized.as_inner().clone();
        exec.validate()?;
        anyhow::ensure!(exec.code_version == code_version);
        let app = App::load_state(app_serialized.as_str())?;
        Ok(ExecutionState {
            next_height: height.next(),
            exec,
            app,
            exec_serialized,
            app_serialized,
        })
    }

    pub fn get_next_height(&self) -> EventHeight {
        self.next_height
    }

    pub fn get_processor_pubkey(&self) -> PublicKey {
        self.exec.processor
    }

    pub(crate) fn increment_height(&mut self) {
        self.next_height = self.next_height.next();
    }

    pub(crate) fn serialize_and_store_framework_state(
        &mut self,
    ) -> Result<&TaggedJson<RawExecutionState>> {
        self.exec_serialized = TaggedJson::new(self.exec.clone())?;
        Ok(&self.exec_serialized)
    }

    pub(crate) fn serialize_and_store_app_state(&mut self) -> Result<&TaggedJson<App::State>> {
        let rendered = App::save_state(&self.app)?;
        let app = App::load_state(&rendered)?;
        self.app_serialized = TaggedJson::from_pair(app, rendered);
        Ok(&self.app_serialized)
    }

    pub(in crate::core) fn get_next_genesis_action(&self) -> Option<GenesisAction> {
        for (chain, config) in &self.exec.chains {
            match config.bridge {
                BridgeContract::NeededCosmosBridge { code_id } => {
                    return Some(GenesisAction::InstantiateCosmos {
                        chain: *chain,
                        code_id,
                        processor: self.exec.processor,
                        listeners: self.exec.listeners.clone(),
                        needed_listeners: self.exec.needed_listeners,
                        executors: self.exec.executors.clone(),
                        needed_executors: self.exec.needed_executors,
                    })
                }
                BridgeContract::Deployed(_) => (),
            }
        }
        None
    }

    pub(in crate::core) fn bridge_created(
        &mut self,
        chain: ExternalChain,
        contract: &str,
    ) -> Result<()> {
        let chain_config = self
            .exec
            .chains
            .get_mut(&chain)
            .with_context(|| format!("bridge_created for unknown chain: {chain:?}"))?;
        match &chain_config.bridge {
            BridgeContract::NeededCosmosBridge { code_id:_ } => (),
            BridgeContract::Deployed(deployed) => anyhow::bail!("Tried to set bridge contract for {chain:?} to {contract}, but we already have deployed contract {deployed}."),
        }
        chain_config.bridge = BridgeContract::Deployed(contract.to_owned());
        Ok(())
    }

    pub(crate) fn get_bridge_contracts(&self) -> &BTreeMap<ExternalChain, ChainConfig> {
        &self.exec.chains
    }
}
