use std::fmt::Display;

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
    },
    LayerAvailable {
        hash: Sha256Hash,
    },
}

impl Display for BlockRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BlockRequest::BlockAtHeight(height) | BlockRequest::BlockWithStateAtHeight(height) => {
                write!(f, "request block {height}")
            }
            BlockRequest::Merkle(hash) => write!(f, "request merkle layer {hash}"),
            BlockRequest::BlockAvailable { height } => {
                write!(f, "notify block {height} available")
            }
            BlockRequest::LayerAvailable { hash } => {
                write!(f, "notify merkle layer {hash} available")
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub(super) enum GossipMessage<App: KolmeApp> {
    RequestBlockHeights(jiff::Timestamp),
    ReportBlockHeight(ReportBlockHeight),
    Notification(Notification<App::Message>),
    BroadcastTx {
        tx: Arc<SignedTransaction<App::Message>>,
    },
    RequestBlockContents {
        height: BlockHeight,
    },
    RequestLayerContents {
        hash: Sha256Hash,
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
            GossipMessage::RequestBlockContents { height } => {
                write!(f, "Request block contents {height}")
            }
            GossipMessage::RequestLayerContents { hash } => {
                write!(f, "Request layer contents {hash}")
            }
        }
    }
}

impl<App: KolmeApp> std::fmt::Debug for GossipMessage<App> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub(super) struct ReportBlockHeight {
    pub(super) next: BlockHeight,
    pub(super) timestamp: jiff::Timestamp,
    pub(super) latest_block: Option<Arc<SignedTaggedJson<LatestBlock>>>,
}

impl<App: KolmeApp> GossipMessage<App> {
    /// returns true if this message was sent (including the case when it was a duplicate)
    /// returns false in the case of the muted error InsufficientPeers
    pub(super) fn publish(self, gossip: &Gossip<App>) {
        tracing::debug!(
            "{}: Publishing message to gossipsub: {self}",
            gossip.local_display_name
        );
        gossip.websockets_manager.publish(&self);
    }
}
