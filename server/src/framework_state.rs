use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct AccountNonce(pub u64);

impl AccountNonce {
    pub fn start() -> Self {
        AccountNonce(0)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Account {
    pub created: Timestamp,
    pub pubkeys: BTreeSet<PublicKey>,
    pub wallets: BTreeSet<Wallet>,
    pub next_nonce: AccountNonce,
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
    pub chains: BTreeMap<ExternalChain, ChainConfig>,
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

    pub(crate) fn is_start(&self) -> bool {
        self.0 == 0
    }
}

#[derive(
    PartialEq, PartialOrd, Ord, Eq, Clone, Debug, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Wallet(String);

pub struct FrameworkState {
    pub raw: RawFrameworkState,
    pub next_event_height: EventHeight,
    pub next_state_height: EventHeight,
    pub pubkeys: BTreeMap<k256::PublicKey, AccountId>,
    pub wallets: HashMap<Wallet, AccountId>,
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

        let mut pubkeys = BTreeMap::new();
        let mut wallets = HashMap::new();

        for (account_id, account) in &raw.accounts {
            for pubkey in &account.pubkeys {
                pubkeys.insert(*pubkey, *account_id);
            }
            for wallet in &account.wallets {
                wallets.insert(wallet.clone(), *account_id);
            }
        }

        Ok(FrameworkState {
            raw,
            next_event_height,
            next_state_height,
            pubkeys,
            wallets,
        })
    }

    pub(crate) fn validate_code_version(&self, code_version: impl AsRef<str>) -> Result<()> {
        let code_version = code_version.as_ref();
        anyhow::ensure!(
            self.raw.code_version == code_version,
            "Could not match discovered code version {} against actual code version {}",
            self.raw.code_version,
            code_version
        );
        Ok(())
    }

    pub fn get_account_id(&self, key: &k256::PublicKey) -> Option<AccountId> {
        self.pubkeys.get(key).copied()
    }
}
