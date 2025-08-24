use std::fmt::Display;

use crate::*;

/// Any message that can be sent over the gossip channels.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(bound(serialize = "", deserialize = ""), rename_all = "snake_case")]
pub(super) enum GossipMessage<App: KolmeApp> {
    /// Provide the latest block information from the processor.
    ProvideLatestBlock {
        latest: Arc<SignedTaggedJson<LatestBlock>>,
    },
    /// Request for the contents of a specific block
    RequestBlock { height: BlockHeight },
    /// Provide a block that was requested.
    ///
    /// If we don't have that block, we return None.
    ProvideBlock {
        height: BlockHeight,
        block: Option<Arc<SignedBlock<App::Message>>>,
    },
    /// Request the contents of a Merkle layer
    RequestLayer { hash: Sha256Hash },
    /// Provide a layer that was requested.
    ///
    /// If we don't have that layer, we return None.
    ProvideLayer {
        hash: Sha256Hash,
        contents: Option<Arc<MerkleLayerContents>>,
    },
    /// Broadcast a transaction throughout the network
    BroadcastTx {
        tx: Arc<SignedTransaction<App::Message>>,
    },
    /// Notification from the processor that a transaction failed.
    FailedTransaction {
        failed: Arc<SignedTaggedJson<FailedTransaction>>,
    },
}

impl<App: KolmeApp> Display for GossipMessage<App> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            GossipMessage::ProvideLatestBlock { latest } => {
                write!(f, "Provide latest block information: {latest:?}")
            }
            GossipMessage::RequestBlock { height } => write!(f, "Request block height {height}"),
            GossipMessage::ProvideBlock { height, block } => {
                write!(f, "Provide block height {height}: {block:?}")
            }
            GossipMessage::RequestLayer { hash } => write!(f, "Request layer {hash}"),
            GossipMessage::ProvideLayer { hash, contents } => {
                write!(f, "Provide layer {hash}: present == {}", contents.is_some())
            }
            GossipMessage::BroadcastTx { tx } => {
                write!(f, "Broadcast {}", tx.hash())
            }
            GossipMessage::FailedTransaction { failed } => {
                write!(
                    f,
                    "Report failed transaction: {}",
                    failed.message.as_inner()
                )
            }
        }
    }
}

impl<App: KolmeApp> std::fmt::Debug for GossipMessage<App> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl<App: KolmeApp> GossipMessage<App> {
    /// returns true if this message was sent (including the case when it was a duplicate)
    /// returns false in the case of the muted error InsufficientPeers
    pub(super) fn publish(self, gossip: &Gossip<App>) {
        tracing::debug!(
            "{}: Publishing message to Gossip: {self}",
            gossip.local_display_name
        );
        gossip.websockets_manager.publish(&self);
    }
}
