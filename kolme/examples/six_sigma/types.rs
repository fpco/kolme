use kolme::{AccountId, AssetId, BlockHeight, Decimal};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AppMessage {
    SetConfig {
        sr_account: AccountId,
        market_funds_account: AccountId,
    },
    AddMarket {
        id: u64,
        asset_id: AssetId,
        name: String,
    },
    PlaceBet {
        wallet: String,
        amount: Decimal,
        market_id: u64,
        outcome: u8,
    },
    SettleMarket {
        market_id: u64,
        outcome: u8,
    },
}

#[derive(PartialEq, serde::Serialize, serde::Deserialize, Debug)]
pub enum LogOutput {
    NewBlock { height: BlockHeight },
    GenesisInstantiation,
}

#[derive(serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
pub enum Config {
    #[default]
    Empty,
    Configured {
        // Strategic reserve from which markets get their funds
        sr_account: AccountId,
        // Account holding funds allocated for markets both from bets and house provisions
        market_funds_account: AccountId,
    },
}
