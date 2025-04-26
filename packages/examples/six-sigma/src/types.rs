use clap::Subcommand;
use std::str::FromStr;

use kolme::{AssetId, BlockHeight, Decimal, MerkleDeserialize, MerkleSerialError, MerkleSerialize};

#[derive(Subcommand)]
pub enum AppComponent {
    /// Run the API server
    ApiServer,
    /// Run the Submitter service
    Submitter,
    /// Run the Processor service
    Processor,
    /// Run the Listener service
    Listener,
    /// Run the Approver service
    Approver,
}

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

#[derive(
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    Default,
    Clone,
    Debug,
    strum::AsRefStr,
    strum::EnumString,
)]
#[strum(serialize_all = "snake_case")]
pub enum AppState {
    #[default]
    Uninitialized,
    Operational,
}

impl MerkleSerialize for AppState {
    fn merkle_serialize(
        &self,
        serializer: &mut kolme::MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(self.as_ref())
    }
}

impl MerkleDeserialize for AppState {
    fn merkle_deserialize(
        deserializer: &mut kolme::MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        AppState::from_str(deserializer.load_str()?).map_err(MerkleSerialError::custom)
    }
}
