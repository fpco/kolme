use std::fmt::Display;

use gossip::KolmeBehaviour;
use libp2p::{
    gossipsub::{Message, PublishError},
    PeerId, Swarm,
};

use crate::*;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
pub(super) enum BlockRequest {
    /// Return the contents of a specific block.
    BlockAtHeight(BlockHeight),
    /// Return both the raw block as well as the full app and framework state to go along with it.
    /// TODO: Fairly certain this now has the exact same behavior as BlockAtHeight, can be removed. Just need to investigate implications on breaking network protocol.
    BlockWithStateAtHeight(BlockHeight),
    /// Request a Merkle layer
    Merkle(Sha256Hash),
    /// Notify a node that the given peer has the given block for transfer.
    BlockAvailable {
        height: BlockHeight,
        #[serde(
            serialize_with = "serialize_peer_id",
            deserialize_with = "deserialize_peer_id"
        )]
        peer: PeerId,
    },
    LayerAvailable {
        hash: Sha256Hash,
        #[serde(
            serialize_with = "serialize_peer_id",
            deserialize_with = "deserialize_peer_id"
        )]
        peer: PeerId,
    },
}

impl Display for BlockRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BlockRequest::BlockAtHeight(height) | BlockRequest::BlockWithStateAtHeight(height) => {
                write!(f, "request block {height}")
            }
            BlockRequest::Merkle(hash) => write!(f, "request merkle layer {hash}"),
            BlockRequest::BlockAvailable { height, peer } => {
                write!(f, "notify block {height} available from {peer}")
            }
            BlockRequest::LayerAvailable { hash, peer } => {
                write!(f, "notify merkle layer {hash} available from {peer}")
            }
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
pub(super) enum BlockResponse<AppMessage: serde::de::DeserializeOwned> {
    Block(Arc<SignedBlock<AppMessage>>),
    /// TODO: Fairly certain this now has the exact same behavior as Block, can be removed. Just need to investigate implications on breaking network protocol.
    BlockWithState {
        block: Arc<SignedBlock<AppMessage>>,
    },
    Merkle {
        hash: Sha256Hash,
        contents: MerkleLayerContents,
    },
    HeightNotFound(BlockHeight),
    MerkleNotFound(Sha256Hash),
    /// Acknowledge a previous request that requires no response information.
    Ack,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) enum GossipMessage<App: KolmeApp> {
    RequestBlockHeights(jiff::Timestamp),
    ReportBlockHeight(ReportBlockHeight),
    Notification(Notification<App::Message>),
    BroadcastTx {
        tx: Arc<SignedTransaction<App::Message>>,
    },
    RequestBlockContents {
        height: BlockHeight,
        #[serde(
            serialize_with = "serialize_peer_id",
            deserialize_with = "deserialize_peer_id"
        )]
        peer: PeerId,
    },
    RequestLayerContents {
        hash: Sha256Hash,
        #[serde(
            serialize_with = "serialize_peer_id",
            deserialize_with = "deserialize_peer_id"
        )]
        peer: PeerId,
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
            GossipMessage::BroadcastTx { tx } => {
                write!(f, "Broadcast {}", tx.hash())
            }
            GossipMessage::RequestBlockContents { height, peer } => {
                write!(f, "Request block contents {height} for {peer}")
            }
            GossipMessage::RequestLayerContents { hash, peer } => {
                write!(f, "Request layer contents {hash} for {peer}")
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
    pub(super) latest_block: Option<Arc<SignedTaggedJson<LatestBlock>>>,
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

    /// returns true if this message was sent (including the case when it was a duplicate)
    /// returns false in the case of the muted error InsufficientPeers
    pub(super) fn publish(
        self,
        gossip: &Gossip<App>,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
    ) -> Result<bool> {
        tracing::debug!(
            "{}: Publishing message to gossipsub: {self}",
            gossip.local_display_name
        );
        // TODO should we put in some retry logic to handle the "InsufficientPeers" case?
        let result = swarm
            .behaviour_mut()
            .gossipsub
            .publish(gossip.gossip_topic.clone(), serde_json::to_vec(&self)?);
        match result {
            Ok(_id) => Ok(true),
            Err(PublishError::Duplicate) => {
                tracing::debug!(
                    "{}: Skipping sending duplicate message",
                    gossip.local_display_name
                );
                Ok(true)
            }
            Err(PublishError::NoPeersSubscribedToTopic) => {
                tracing::info!(
                    "{}: Not enough peers to send this message to",
                    gossip.local_display_name
                );
                Ok(false)
            }
            Err(err) => Err(err).with_context(|| {
                format!(
                    "{}: Unable to publish a gossipsub message: {self}",
                    gossip.local_display_name
                )
            }),
        }
    }
}
