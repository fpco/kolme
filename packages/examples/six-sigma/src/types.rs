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
        messages: Vec<LoggedMessage>,
    },
    GenesisInstantiation,
}

// kolme::Message with details only necessary for tests
#[derive(PartialEq, serde::Serialize, serde::Deserialize, Debug)]
pub enum LoggedMessage {
    Genesis,
    App(AppMessage),
    Listener(LoggedBridgeEvent),
    Approve,
    ProcessorApprove,
    Auth,
    Bank,
    Admin,
}

impl From<kolme::Message<AppMessage>> for LoggedMessage {
    fn from(msg: kolme::Message<AppMessage>) -> Self {
        match msg {
            kolme::Message::Genesis(_) => LoggedMessage::Genesis,
            kolme::Message::App(app_message) => LoggedMessage::App(app_message),
            kolme::Message::Listener { event, .. } => LoggedMessage::Listener(event.into()),
            kolme::Message::Approve { .. } => LoggedMessage::Approve,
            kolme::Message::ProcessorApprove { .. } => LoggedMessage::ProcessorApprove,
            kolme::Message::Auth(_) => LoggedMessage::Auth,
            kolme::Message::Bank(_) => LoggedMessage::Bank,
            kolme::Message::Admin(_) => LoggedMessage::Admin,
        }
    }
}

#[derive(PartialEq, serde::Serialize, serde::Deserialize, Debug)]
pub enum LoggedBridgeEvent {
    Instantiated,
    Regular,
    Signed,
}

impl From<kolme::BridgeEvent> for LoggedBridgeEvent {
    fn from(event: kolme::BridgeEvent) -> Self {
        match event {
            kolme::BridgeEvent::Instantiated { .. } => LoggedBridgeEvent::Instantiated,
            kolme::BridgeEvent::Regular { .. } => LoggedBridgeEvent::Regular,
            kolme::BridgeEvent::Signed { .. } => LoggedBridgeEvent::Signed,
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
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        AppState::from_str(deserializer.load_str()?).map_err(MerkleSerialError::custom)
    }
}
