use crate::*;

/// Raw framework state that can be serialized to the database.
#[derive(serde::Serialize, serde::Deserialize)]
struct RawExecutionState {
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
    exec_serialized: Vec<u8>,
    app_serialized: Vec<u8>,
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
        let exec_serialized = serde_json::to_vec(&exec)?;
        let app_serialized = App::save_state(&app)?;
        Ok(ExecutionState {
            next_height: EventHeight::start(),
            exec,
            app,
            exec_serialized,
            app_serialized,
        })
    }

    pub(super) fn load(
        _app: &App,
        ExecutionStreamState {
            height,
            framework: exec_serialized,
            app: app_serialized,
        }: ExecutionStreamState,
        code_version: &str,
    ) -> Result<Self> {
        let exec = serde_json::from_slice::<RawExecutionState>(&exec_serialized)?;
        exec.validate()?;
        anyhow::ensure!(exec.code_version == code_version);
        let app = App::load_state(&app_serialized)?;
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
}
