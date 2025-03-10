use std::collections::BTreeMap;

use crate::AssetId;

#[derive(
    serde::Serialize, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug,
)]
pub enum ExternalChain {
    OsmosisTestnet,
    NeutronTestnet,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ChainConfig {
    pub assets: BTreeMap<String, AssetConfig>,
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
