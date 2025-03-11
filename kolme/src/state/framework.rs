use crate::*;

/// Raw framework state that can be serialized to the database.
#[derive(serde::Serialize, serde::Deserialize)]
struct RawFrameworkState {
    code_version: String,
    processor: PublicKey,
    listeners: BTreeSet<PublicKey>,
    needed_listeners: usize,
    executors: BTreeSet<PublicKey>,
    needed_executors: usize,
    chains: BTreeMap<ExternalChain, ChainConfig>,
    // TODO: add in balances, something like this: pub balances: BTreeMap<AccountId, BTreeMap<AssetId, Decimal>>
}

impl RawFrameworkState {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.executors.len() >= self.needed_executors);
        anyhow::ensure!(self.needed_executors > 0);
        Ok(())
    }
}

pub struct FrameworkState {
    raw: RawFrameworkState,
    /// Serialized format stored to quickly reload if we need to roll back event processing.
    serialized: Vec<u8>,
    next_state_height: EventHeight,
}
