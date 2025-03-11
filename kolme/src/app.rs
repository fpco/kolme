use std::collections::{BTreeMap, BTreeSet};

use crate::*;

pub trait KolmeApp {
    /// The state maintained in memory, may contain helper
    /// data structures for more efficient operations.
    type State;

    /// Messages that are understood by this app.
    type Message: serde::Serialize + serde::de::DeserializeOwned;

    /// Initial chain information.
    fn genesis_info() -> GenesisInfo;

    /// Serialize to a format that can be stored in the database.
    ///
    /// Note that it may be useful to keep extra data for efficiency
    /// within the state, and then omit that data during serialization.
    fn save_state(state: &Self::State) -> Result<Vec<u8>>;

    /// Load state previously serialized with [Self::save_state].
    fn load_state(v: &[u8]) -> Result<Self::State>;

    /// Generate a blank state.
    fn new_state() -> Result<Self::State>;
}

/// Information defining the initial state of an app.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct GenesisInfo {
    /// Unique identifier for this application, never changes.
    pub kolme_ident: String,
    /// Public key of the processor for this app
    pub processor: PublicKey,
    /// Public keys of the listeners for this app
    pub listeners: BTreeSet<PublicKey>,
    /// How many of the listeners are needed to approve a reported bridge event?
    pub needed_listeners: usize,
    /// Public keys of the executors for this app
    pub executors: BTreeSet<PublicKey>,
    /// How many of the executors are needed to approve a bridge action?
    pub needed_executors: usize,
    /// Initial configuration of different chains
    pub chains: BTreeMap<ExternalChain, ChainConfig>,
}

impl GenesisInfo {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.executors.len() >= self.needed_executors);
        anyhow::ensure!(self.needed_executors > 0);
        Ok(())
    }
}
