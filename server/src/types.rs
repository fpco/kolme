#[derive(
    serde::Serialize, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug,
)]
pub enum ExternalChain {
    OsmosisTestnet,
    NeutronTestnet,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum BridgeContract {
    NeededCosmosBridge { code_id: u64 },
    Deployed(String),
}
