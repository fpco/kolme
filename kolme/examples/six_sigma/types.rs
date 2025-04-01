use kolme::{AssetId, BlockHeight, Decimal};

#[derive(PartialEq, serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AppMessage {
    SendFunds {
        asset_id: AssetId,
        amount: Decimal,
    },
    Init,
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
    NewBlock {
        height: BlockHeight,
        messages: Vec<LogMessage>,
    },
    GenesisInstantiation,
}

// kolme::Message with details only necessary for tests
#[derive(PartialEq, serde::Serialize, serde::Deserialize, Debug)]
pub enum LogMessage {
    Genesis,
    App(AppMessage),
    Listener(LogBridgeEvent),
    Approve,
    ProcessorApprove,
    Auth,
    Bank,
}

impl From<kolme::Message<AppMessage>> for LogMessage {
    fn from(msg: kolme::Message<AppMessage>) -> Self {
        match msg {
            kolme::Message::Genesis(_) => LogMessage::Genesis,
            kolme::Message::App(app_message) => LogMessage::App(app_message),
            kolme::Message::Listener { event, .. } => LogMessage::Listener(event.into()),
            kolme::Message::Approve { .. } => LogMessage::Approve,
            kolme::Message::ProcessorApprove { .. } => LogMessage::ProcessorApprove,
            kolme::Message::Auth(_) => LogMessage::Auth,
            kolme::Message::Bank(_) => LogMessage::Bank,
        }
    }
}

#[derive(PartialEq, serde::Serialize, serde::Deserialize, Debug)]
pub enum LogBridgeEvent {
    Instantiated,
    Regular,
    Signed,
}

impl From<kolme::BridgeEvent> for LogBridgeEvent {
    fn from(event: kolme::BridgeEvent) -> Self {
        match event {
            kolme::BridgeEvent::Instantiated { .. } => LogBridgeEvent::Instantiated,
            kolme::BridgeEvent::Regular { .. } => LogBridgeEvent::Regular,
            kolme::BridgeEvent::Signed { .. } => LogBridgeEvent::Signed,
        }
    }
}

#[derive(PartialEq, serde::Serialize, serde::Deserialize, Default, Clone, Debug)]
pub enum AppState {
    #[default]
    Unitialized,
    Operational,
}
