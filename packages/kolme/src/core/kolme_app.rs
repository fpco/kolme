use crate::*;

pub trait KolmeApp: Send + Sync + Clone + 'static {
    /// The state maintained in memory, may contain helper
    /// data structures for more efficient operations.
    type State: Send + Sync + Clone + 'static;

    /// Messages that are understood by this app.
    type Message: serde::Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + Clone
        + std::fmt::Debug
        + 'static;

    /// Initial chain information.
    fn genesis_info() -> GenesisInfo;

    /// Serialize to a format that can be stored in the database.
    ///
    /// Note that it may be useful to keep extra data for efficiency
    /// within the state, and then omit that data during serialization.
    fn save_state(state: &Self::State) -> Result<String>;

    /// Load state previously serialized with [Self::save_state].
    fn load_state(v: &str) -> Result<Self::State>;

    /// Generate a blank state.
    fn new_state() -> Result<Self::State>;

    /// Execute a message.
    fn execute(
        &self,
        ctx: &mut ExecutionContext<Self>,
        msg: &Self::Message,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}

pub trait KolmeDataRequest<App>:
    serde::Serialize + serde::de::DeserializeOwned + PartialEq
{
    type Response: serde::Serialize + serde::de::DeserializeOwned;

    /// Do an initial load of the data
    #[allow(async_fn_in_trait)]
    async fn load(self, app: &App) -> Result<Self::Response>;

    /// Validate previously loaded data
    #[allow(async_fn_in_trait)]
    async fn validate(self, app: &App, prev_res: &Self::Response) -> Result<()>;
}
