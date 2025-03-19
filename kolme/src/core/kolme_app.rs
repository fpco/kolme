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
    #[allow(async_fn_in_trait)]
    async fn execute(&self, ctx: &mut ExecutionContext<Self>, msg: &Self::Message) -> Result<()>;
}
