use std::fmt::Display;

use crate::*;
use k256::{ecdsa::Signature, PublicKey};

#[derive(
    serde::Serialize, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Debug, Hash,
)]
pub enum ExternalChain {
    OsmosisTestnet,
    NeutronTestnet,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ChainConfig {
    pub assets: BTreeMap<AssetName, AssetConfig>,
    pub bridge: BridgeContract,
}
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AssetConfig {
    pub decimals: u8,
    pub asset_id: AssetId,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum BridgeContract {
    NeededCosmosBridge { code_id: u64 },
    Deployed(String),
}

#[derive(serde::Serialize)]
pub enum GenesisAction {
    InstantiateCosmos {
        chain: ExternalChain,
        code_id: u64,
        processor: PublicKey,
        listeners: BTreeSet<PublicKey>,
        needed_listeners: usize,
        executors: BTreeSet<PublicKey>,
        needed_executors: usize,
    },
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

impl Display for EventHeight {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
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

/// An event that is signed by the processor.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
pub struct SignedEvent<AppMessage>(pub SignedTaggedJson<ApprovedEvent<AppMessage>>);

/// An event that has been accepted by the processor.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound = "AppMessage: serde::de::DeserializeOwned")]
pub struct ApprovedEvent<AppMessage> {
    pub event: ProposedEvent<AppMessage>,
    pub timestamp: Timestamp,
    pub processor: PublicKey,
    pub height: EventHeight,
    pub parent: Sha256Hash,
}

/// A proposed event from a client, not yet added to the stream
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound = "AppMessage: serde::de::DeserializeOwned")]
pub struct ProposedEvent<AppMessage>(pub SignedTaggedJson<EventPayload<AppMessage>>);

impl<AppMessage: serde::Serialize> ProposedEvent<AppMessage> {
    pub fn validate_signature(&self) -> Result<()> {
        let pubkey = self.0.verify_signature()?;
        anyhow::ensure!(pubkey == self.0.message.as_inner().pubkey);
        Ok(())
    }

    pub fn ensure_is_genesis(&self) -> Result<()> {
        anyhow::ensure!(self.0.message.as_inner().messages.len() == 1);
        anyhow::ensure!(matches!(
            self.0.message.as_inner().messages[0],
            EventMessage::Genesis(_)
        ));
        Ok(())
    }

    pub fn ensure_no_genesis(&self) -> Result<()> {
        for msg in &self.0.message.as_inner().messages {
            anyhow::ensure!(!matches!(msg, EventMessage::Genesis(_)));
        }
        Ok(())
    }
}

/// The content of an event, sent by a client to be included in the event series.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct EventPayload<AppMessage> {
    pub pubkey: PublicKey,
    pub nonce: AccountNonce,
    pub created: Timestamp,
    pub messages: Vec<EventMessage<AppMessage>>,
}

impl<AppMessage: serde::Serialize> EventPayload<AppMessage> {
    pub fn sign(self, key: &k256::SecretKey) -> Result<ProposedEvent<AppMessage>> {
        Ok(ProposedEvent(TaggedJson::new(self)?.sign(key)?))
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum EventMessage<AppMessage> {
    Genesis(GenesisInfo),
    BridgeCreated(BridgeCreated),
    App(AppMessage),
    Listener(ListenerMessage),
    Auth(AuthMessage),
    // TODO Bank, with things like
    // Transfer {
    //     asset: AssetId,
    //     amount: Decimal,
    //     dest: AccountId,
    // }

    // TODO: outgoing actions

    // TODO: admin actions: update code version, change processor/listeners/executors (need to update contracts too), modification to chain values (like asset definitions)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum ListenerMessage {
    Deposit {
        asset: String,
        wallet: String,
        amount: u128,
    },
    /// Only include the bare-bones necessary to bootstrap into the auth system
    AddPublicKey { wallet: String, key: String },
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum AuthMessage {
    AddPublicKey { key: PublicKey },
    RemovePublicKey { key: PublicKey },
    AddWallet { wallet: String },
    RemoveWallet { wallet: String },
}

/// Information defining the initial state of an app.
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct GenesisInfo {
    /// Unique identifier for this application, never changes.
    pub kolme_ident: String,
    /// Public key of the processor for this app
    pub processor: PublicKey,
    /// Public keys of the listeners for this app
    pub listeners: BTreeSet<PublicKey>,
    /// How many of the listeners are needed to approve a reported bridge event?
    pub needed_listeners: usize,
    /// Public keys of the executors for this app
    pub executors: BTreeSet<PublicKey>,
    /// How many of the executors are needed to approve a bridge action?
    pub needed_executors: usize,
    /// Initial configuration of different chains
    pub chains: BTreeMap<ExternalChain, ChainConfig>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct BridgeCreated {
    pub chain: ExternalChain,
    pub contract: String,
}

impl GenesisInfo {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.listeners.len() >= self.needed_listeners);
        anyhow::ensure!(self.needed_listeners > 0);
        anyhow::ensure!(self.executors.len() >= self.needed_executors);
        anyhow::ensure!(self.needed_executors > 0);
        Ok(())
    }
}

/// An executed event that is signed by the processor.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SignedExec {
    /// An [ExecutedEvent], see [SignedEvent::event] for details.
    pub exec: TaggedJson<ExecutedEvent>,
    pub signature: Signature,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExecutedEvent {
    pub height: EventHeight,
    pub timestamp: Timestamp,
    // FIXME pub event_hash: EventHash
    // pub previous_executed_event: Option<ExecHash>
    pub framework_state: Sha256Hash,
    pub app_state: Sha256Hash,
    pub loads: Vec<ExecLoad>,
}

#[derive(Default)]
pub(crate) struct MessageOutput {
    pub(crate) logs: Vec<String>,
    pub(crate) loads: Vec<ExecLoad>,
    pub(crate) actions: Vec<ExecAction>,
}

/// Input and output for a single data load.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExecLoad {
    /// Description of the query
    pub query: String,
    /// The resulting value
    pub response: String,
}

/// A specific action to be taken as a result of an execution.
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ExecAction {
    Transfer {
        recipient: String,
        funds: Vec<AssetAmount>,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AssetAmount {
    pub id: AssetId,
    pub amount: u128, // FIXME use a Decimal representation
}

/// Notifications that can come from the Kolme framework to components.
#[derive(Clone, Debug)]
pub enum Notification {
    NewEvent(EventHeight),
    NewExec(EventHeight),
    GenesisInstantiation {
        chain: ExternalChain,
        contract: String,
    },
}
