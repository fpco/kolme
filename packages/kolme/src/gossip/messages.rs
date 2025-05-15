use std::fmt::Display;

use gossip::KolmeBehaviour;
use libp2p::{gossipsub::Message, PeerId, Swarm};

use crate::*;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
pub(super) enum BlockRequest {
    /// Return the contents of a specific block.
    BlockAtHeight(BlockHeight),
    /// Return both the raw block as well as the full app and framework state to go along with it.
    BlockWithStateAtHeight(BlockHeight),
}

#[allow(clippy::large_enum_variant)]
#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
pub(super) enum BlockResponse<AppMessage: serde::de::DeserializeOwned> {
    Block(Arc<SignedBlock<AppMessage>>),
    BlockWithState {
        block: Arc<SignedBlock<AppMessage>>,
        framework_state: Arc<MerkleContents>,
        app_state: Arc<MerkleContents>,
        logs: Arc<MerkleContents>,
    },
    HeightNotFound(BlockHeight),
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) enum GossipMessage<App: KolmeApp> {
    RequestBlockHeights(jiff::Timestamp),
    ReportBlockHeight(ReportBlockHeight),
    Notification(Notification<App::Message>),
    BroadcastTx {
        tx: Arc<SignedTransaction<App::Message>>,
        timestamp: Timestamp,
    },
}

impl<App: KolmeApp> Display for GossipMessage<App> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            GossipMessage::RequestBlockHeights(timestamp) => {
                write!(f, "Request block heights at {timestamp}")
            }
            GossipMessage::ReportBlockHeight(report_block_height) => {
                write!(f, "Report block height: {report_block_height:?}")
            }
            GossipMessage::Notification(notification) => {
                write!(f, "Notification: {notification:?}")
            }
            GossipMessage::BroadcastTx { tx, timestamp } => {
                write!(f, "Broadcast {} @ {timestamp}", tx.hash())
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(super) struct ReportBlockHeight {
    pub(super) next: BlockHeight,
    #[serde(
        serialize_with = "serialize_peer_id",
        deserialize_with = "deserialize_peer_id"
    )]
    pub(super) peer: PeerId,
    pub(super) timestamp: jiff::Timestamp,
}
fn serialize_peer_id<S>(peer_id: &PeerId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&peer_id.to_base58())
}

fn deserialize_peer_id<'de, D>(deserializer: D) -> Result<PeerId, D::Error>
where
    D: serde::Deserializer<'de>,
{
    <String as serde::Deserialize>::deserialize(deserializer)?
        .parse()
        .map_err(serde::de::Error::custom)
}

impl<App: KolmeApp> GossipMessage<App> {
    pub(super) fn parse(gossip: &Gossip<App>, message: Message) -> Result<Self> {
        anyhow::ensure!(
            message.topic == gossip.gossip_topic.hash(),
            "Unknown gossipsub topic"
        );

        serde_json::from_slice(&message.data).context("Unable to parse gossipsub message")
    }

    pub(super) async fn publish(
        self,
        gossip: &Gossip<App>,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
    ) -> Result<()> {
        // TODO should we put in some retry logic to handle the "InsufficientPeers" case?
        swarm
            .behaviour_mut()
            .gossipsub
            .publish(gossip.gossip_topic.clone(), serde_json::to_vec(&self)?)
            .map(|_| ())
            .with_context(|| format!("Unable to publish a gossipsub message: {self}"))
    }
}
