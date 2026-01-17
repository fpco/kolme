use crate::*;

pub trait KolmeApp: Send + Sync + Clone + 'static {
    /// The state maintained in memory, may contain helper
    /// data structures for more efficient operations.
    type State: MerkleSerializeRaw
        + MerkleDeserializeRaw
        + Send
        + Sync
        + Clone
        + std::fmt::Debug
        + 'static;

    /// Messages that are understood by this app.
    type Message: serde::Serialize
        + serde::de::DeserializeOwned
        + Send
        + Sync
        + Clone
        + std::fmt::Debug
        + 'static;

    /// Initial chain information.
    fn genesis_info(&self) -> &GenesisInfo;

    /// Generate a blank state.
    fn new_state(&self) -> Result<Self::State, KolmeError>;

    /// Execute a message.
    fn execute(
        &self,
        ctx: &mut ExecutionContext<Self>,
        msg: &Self::Message,
    ) -> impl std::future::Future<Output = Result<(), KolmeError>> + Send;
}

#[derive(thiserror::Error, Debug)]
pub enum KolmeDataError {
    #[error("Data validation failed")]
    ValidationFailed,

    #[error("Data mismatch")]
    Mismatch,

    #[error("Invalid data request")]
    InvalidRequest,
}

pub trait KolmeDataRequest<App>:
    serde::Serialize + serde::de::DeserializeOwned + PartialEq
{
    type Response: serde::Serialize + serde::de::DeserializeOwned;

    /// Do an initial load of the data
    #[allow(async_fn_in_trait)]
    async fn load(self, app: &App) -> Result<Self::Response, KolmeDataError>;

    /// Validate previously loaded data
    #[allow(async_fn_in_trait)]
    async fn validate(self, app: &App, prev_res: &Self::Response) -> Result<(), KolmeDataError>;
}
