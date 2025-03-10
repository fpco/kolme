use std::collections::{BTreeMap, BTreeSet};

use k256::PublicKey;

use crate::prelude::*;

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct AssetId(pub u64);

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug,
)]
pub struct AssetName(pub String);

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct AccountId(pub u64);

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug,
)]
pub struct Account {
    pub created: Timestamp,
    pub authorities: Vec<AccountAuthority>,
    // TODO: add in balances, something like this: pub balances: BTreeMap<AssetId, Decimal>
}

#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub enum AccountAuthority {
    // TODO raw public key
    // TODO Ethereum and Solana addresses
    Cosmos(cosmos::Address),
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RawFrameworkState {
    // We intentionally do not include things like height in here.
    // This allows us to bypass unnecessary updates to state, allowing
    // for more shared storage.
    pub assets: BTreeMap<AssetName, AssetId>,
    pub accounts: BTreeMap<AccountId, Account>,
    /// Unique identifier for this deployment
    pub kolme_ident: String,
    pub code_version: String,
    pub processor: PublicKey,
    pub listeners: BTreeSet<PublicKey>,
    pub needed_listeners: usize,
    pub executors: BTreeSet<PublicKey>,
    pub needed_executors: usize,
    pub bridges: BTreeMap<ExternalChain, BridgeContract>,
}

impl RawFrameworkState {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.executors.len() >= self.needed_executors);
        anyhow::ensure!(self.needed_executors > 0);
        Ok(())
    }
}

/// Height of an event
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct EventHeight(pub u64);
impl EventHeight {
    pub fn next(self) -> EventHeight {
        EventHeight(self.0 + 1)
    }

    pub fn start() -> EventHeight {
        EventHeight(0)
    }
}

pub struct FrameworkState {
    raw: RawFrameworkState,
    next_event_height: EventHeight,
    next_state_height: EventHeight,
}

impl RawFrameworkState {}

impl FrameworkState {
    pub async fn load<App: KolmeApp>(
        _app: &App,
        last_event_height: EventHeight,
        last_state_height: EventHeight,
        payload: &[u8],
    ) -> Result<Self> {
        let raw = serde_json::from_slice::<RawFrameworkState>(payload)?;
        anyhow::ensure!(raw.kolme_ident == App::kolme_ident());
        Self::from_raw(raw, last_event_height.next(), last_state_height.next())
    }

    pub(crate) fn new(
        last_event_height: Option<EventHeight>,
        raw: RawFrameworkState,
    ) -> Result<Self> {
        Self::from_raw(
            raw,
            last_event_height.map_or_else(EventHeight::start, EventHeight::next),
            EventHeight::start(),
        )
    }

    fn from_raw(
        raw: RawFrameworkState,
        next_event_height: EventHeight,
        next_state_height: EventHeight,
    ) -> Result<FrameworkState> {
        anyhow::ensure!(next_event_height >= next_state_height);
        raw.validate()?;
        Ok(FrameworkState {
            raw,
            next_event_height,
            next_state_height,
        })
    }

    pub(crate) fn validate_code_version(&self, code_version: impl AsRef<str>) -> Result<()> {
        anyhow::ensure!(self.raw.code_version == code_version.as_ref());
        Ok(())
    }
}
