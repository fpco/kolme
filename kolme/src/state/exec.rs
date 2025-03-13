use crate::*;

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
        }: ExecutionStreamState<App>,
        code_version: &str,
    ) -> Result<Self> {
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
}
