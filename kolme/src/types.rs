use crate::*;

#[derive(
    serde::Serialize, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug,
)]
pub enum ExternalChain {
    OsmosisTestnet,
    NeutronTestnet,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ChainConfig {
    pub assets: BTreeMap<AssetName, AssetConfig>,
    pub bridge: BridgeContract,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct AssetConfig {
    pub decimals: u8,
    pub asset_id: AssetId,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum BridgeContract {
    NeededCosmosBridge { code_id: u64 },
    Deployed(String),
}

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

    pub(crate) fn increment(&mut self) {
        self.0 += 1;
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

    pub(crate) fn try_into_i64(self) -> Result<i64> {
        self.0.try_into().map_err(anyhow::Error::from)
    }
}

impl TryFrom<i64> for EventHeight {
    type Error = anyhow::Error;

    fn try_from(value: i64) -> Result<Self> {
        value.try_into().map_err(anyhow::Error::from).map(Self)
    }
}

/// Blockchain wallet address.
///
/// To allow support for arbitrary chains, we represent this as a simple [String].
///
/// TODO: Do we need to be worried about differences in case representation, e.g. for EVM hex addresses?
#[derive(
    PartialEq, PartialOrd, Ord, Eq, Clone, Debug, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Wallet(pub String);

/// Monotonically increasing identifier for events coming from a bridge contract.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct BridgeEventId(pub u64);

/// Monotonically increasing identifier for actions sent to a bridge contract.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct BridgeActionId(pub u64);
