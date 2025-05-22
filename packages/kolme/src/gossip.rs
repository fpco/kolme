mod messages;

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    time::Duration,
};

use crate::*;
use messages::*;

use libp2p::{
    futures::StreamExt,
    gossipsub::{self, IdentTopic},
    mdns, noise,
    request_response::{ProtocolSupport, ResponseChannel},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, StreamProtocol, Swarm, SwarmBuilder,
};
use tokio::sync::{broadcast::error::RecvError, Mutex};

pub use libp2p::{identity::Keypair, Multiaddr, PeerId};

/// A component that retrieves notifications from the network and broadcasts our own notifications back out.
pub struct Gossip<App: KolmeApp> {
    kolme: Kolme<App>,
    swarm: Mutex<Swarm<KolmeBehaviour<App::Message>>>,
    gossip_topic: IdentTopic,
    sync_mode: SyncMode,
    // FIXME resolve this when we implement data load validation logic
    #[allow(dead_code)]
    data_load_validation: DataLoadValidation,
    local_peer_id: PeerId,
    /// Trigger a broadcast of our latest block height.
    trigger_broadcast_height: tokio::sync::watch::Sender<u64>,
    /// Switches to true once we have our first success received message
    watch_network_ready: tokio::sync::watch::Sender<bool>,
}

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
struct KolmeBehaviour<AppMessage: serde::de::DeserializeOwned + Send + Sync + 'static> {
    gossipsub: gossipsub::Behaviour,
    request_response:
        libp2p::request_response::cbor::Behaviour<BlockRequest, BlockResponse<AppMessage>>,
    mdns: mdns::tokio::Behaviour,
    kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
}

#[derive(Default)]
pub struct GossipBuilder {
    keypair: Option<Keypair>,
    bootstrap: Vec<(PeerId, Multiaddr)>,
    disable_quic: bool,
    disable_tcp: bool,
    disable_ip4: bool,
    disable_ip6: bool,
    listen_ports: Vec<u16>,
    sync_mode: SyncMode,
    data_load_validation: DataLoadValidation,
}

/// How block data is synchronized.
///
/// Default: [SyncMode::StateTransfer]
#[derive(Default, Debug)]
pub enum SyncMode {
    /// Allow state transfer always (aka fast sync).
    ///
    /// Requires trust in the processor to only produce valid blocks, no verification occurs on our node.
    #[default]
    StateTransfer,
    /// Allow state transfer for version upgrades, but otherwise use block sync.
    StateTransferForUpgrade,
    /// Always do block sync, verifying each new block
    BlockTransfer,
}

/// Whether we validate data loads during block processing.
///
/// Default: [DataLoadValidation::ValidateDataLoads].
#[derive(Default)]
pub enum DataLoadValidation {
    /// Validate that data loaded during a block is accurate.
    ///
    /// This may involve additional I/O, such as making HTTP requests.
    #[default]
    ValidateDataLoads,
    /// Trust that the loaded data is accurate.
    TrustDataLoads,
}

impl GossipBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_bootstrap(mut self, peer: PeerId, address: Multiaddr) -> Self {
        self.bootstrap.push((peer, address));
        self
    }

    pub fn set_keypair(mut self, keypair: Keypair) -> Self {
        self.keypair = Some(keypair);
        self
    }

    pub fn disable_quic(mut self) -> Self {
        self.disable_quic = true;
        self
    }

    pub fn disable_tcp(mut self) -> Self {
        self.disable_tcp = true;
        self
    }

    pub fn disable_ip4(mut self) -> Self {
        self.disable_ip4 = true;
        self
    }

    pub fn disable_ip6(mut self) -> Self {
        self.disable_ip6 = true;
        self
    }

    /// Add a listen port
    ///
    /// If none are provided, a random port is selected per interface
    pub fn add_listen_port(mut self, port: u16) -> Self {
        self.listen_ports.push(port);
        self
    }

    /// Set the sync mode and data validation rules.
    pub fn set_sync_mode(
        mut self,
        sync_mode: SyncMode,
        data_load_validation: DataLoadValidation,
    ) -> Self {
        self.sync_mode = sync_mode;
        self.data_load_validation = data_load_validation;
        self
    }

    pub async fn build<App: KolmeApp>(self, kolme: Kolme<App>) -> Result<Gossip<App>> {
        let builder = match self.keypair {
            Some(keypair) => SwarmBuilder::with_existing_identity(keypair),
            None => SwarmBuilder::with_new_identity(),
        };
        let mut swarm = builder
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            .with_behaviour(|key| {
                tracing::info!(
                    "Creating new gossip, running as peer ID: {}",
                    key.public().to_peer_id()
                );

                // To content-address message, we can take the hash of message and use it as an ID.
                let message_id_fn = |message: &gossipsub::Message| {
                    let mut s = DefaultHasher::new();
                    message.data.hash(&mut s);
                    gossipsub::MessageId::from(s.finish().to_string())
                };

                // Set a custom gossipsub configuration
                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
                    .validation_mode(gossipsub::ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message
                    // signing)
                    .message_id_fn(message_id_fn) // content-address messages. No two messages of the same content will be propagated.
                    .build()
                    .map_err(anyhow::Error::from)?;

                // build a gossipsub network behaviour
                let gossipsub = gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )?;

                let mdns = mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    key.public().to_peer_id(),
                )?;
                let request_response = libp2p::request_response::cbor::Behaviour::new(
                    [(
                        StreamProtocol::new("/request-block/1"),
                        ProtocolSupport::Full,
                    )],
                    libp2p::request_response::Config::default(),
                );

                // Kademlia
                let kademlia_config = libp2p::kad::Config::default();
                let store = libp2p::kad::store::MemoryStore::new(key.public().to_peer_id());
                let mut kademlia = libp2p::kad::Behaviour::with_config(
                    key.public().to_peer_id(),
                    store,
                    kademlia_config,
                );
                for (peer, address) in self.bootstrap {
                    kademlia.add_address(&peer, address);
                }

                Ok(KolmeBehaviour {
                    gossipsub,
                    mdns,
                    request_response,
                    kademlia,
                })
            })?
            .build();

        // Create the Gossipsub topics
        let gossip_topic = gossipsub::IdentTopic::new("/kolme-gossip/1.0");
        // And subscribe
        swarm.behaviour_mut().gossipsub.subscribe(&gossip_topic)?;

        // Begin listening based on the config
        fn add_listen<AppMessage: serde::de::DeserializeOwned + Send + Sync>(
            swarm: &mut Swarm<KolmeBehaviour<AppMessage>>,
            is_quic: bool,
            is_ip6: bool,
            port: u16,
        ) -> Result<()> {
            swarm.listen_on(
                format!(
                    "{}/{}/{port}{}",
                    if is_ip6 { "/ip6/::" } else { "/ip4/0.0.0.0" },
                    if is_quic { "udp" } else { "tcp" },
                    if is_quic { "/quic-v1" } else { "" }
                )
                .parse()?,
            )?;
            Ok(())
        }
        let add_port = |swarm: &mut Swarm<KolmeBehaviour<App::Message>>, port: u16| -> Result<()> {
            // Listen on all interfaces and whatever port the OS assigns
            if !self.disable_quic && !self.disable_ip4 {
                add_listen(swarm, true, false, port)?;
            }
            if !self.disable_tcp && !self.disable_ip4 {
                add_listen(swarm, false, false, port)?;
            }
            if !self.disable_quic && !self.disable_ip6 {
                add_listen(swarm, true, true, port)?;
            }
            if !self.disable_tcp && !self.disable_ip6 {
                add_listen(swarm, false, true, port)?;
            }
            Ok(())
        };

        if self.listen_ports.is_empty() {
            add_port(&mut swarm, 0)?;
        } else {
            for port in &self.listen_ports {
                add_port(&mut swarm, *port)?;
            }
        }

        let (trigger_broadcast_height, _) = tokio::sync::watch::channel(0);
        let (watch_network_ready, _) = tokio::sync::watch::channel(false);
        let local_peer_id = *swarm.local_peer_id();

        Ok(Gossip {
            kolme,
            swarm: Mutex::new(swarm),
            gossip_topic,
            sync_mode: self.sync_mode,
            data_load_validation: self.data_load_validation,
            local_peer_id,
            trigger_broadcast_height,
            watch_network_ready,
        })
    }
}

impl<App: KolmeApp> Gossip<App> {
    pub fn peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    pub fn subscribe_network_ready(&self) -> tokio::sync::watch::Receiver<bool> {
        self.watch_network_ready.subscribe()
    }

    pub async fn run(self) -> Result<()> {
        let mut subscription = self.kolme.subscribe();
        let mut swarm = self.swarm.lock().await;

        let mut network_ready = self.watch_network_ready.subscribe();

        let (peers_with_blocks_tx, mut peers_with_blocks_rx) = tokio::sync::mpsc::channel(16);

        // Interval for broadcasting our block height
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut trigger_broadcast_height = self.trigger_broadcast_height.subscribe();

        let mut mempool_additions = self.kolme.subscribe_mempool_additions();

        loop {
            tokio::select! {
                // When the network switches to ready, request everyone's block height.
                _ = network_ready.changed() => self.request_block_heights(&mut swarm).await,
                // Our local Kolme generated a notification to be sent through the
                // rest of the p2p network
                notification = subscription.recv() =>
                    self.handle_notification(&mut swarm, notification).await,
                // A new event was generated from the p2p network
                event = swarm.select_next_some() => self.handle_event(&mut swarm, event, &peers_with_blocks_tx).await,
                // A peer reported a known height higher than we have, so
                // try to synchronize with it
                report_block_height = peers_with_blocks_rx.recv() => self.catch_up(&mut swarm,report_block_height).await,
                // Periodically notify the p2p network of our latest block height
                // Also use this time to broadcast any transactions from the mempool.
                _ = interval.tick() => {
                    self.broadcast_latest_block(&mut swarm).await;
                    self.broadcast_mempool_entries(&mut swarm).await;
                }
                // When we're specifically triggered for it, also notify for latest block height
                _ = trigger_broadcast_height.changed() => self.broadcast_latest_block(&mut swarm).await,
                // And any time we add something new to the mempool, broadcast all items.
                _ = mempool_additions.changed() => self.broadcast_mempool_entries(&mut swarm).await,
            }
        }
    }

    async fn request_block_heights(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        // First thing to do: ask the network to tell us about their latest
        // block heights.
        if let Err(e) = GossipMessage::RequestBlockHeights(jiff::Timestamp::now())
            .publish(self, swarm)
            .await
        {
            tracing::error!("Unable to request block heights: {e:?}");
        }
    }

    async fn broadcast_latest_block(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        if let Err(e) = GossipMessage::ReportBlockHeight(ReportBlockHeight {
            next: self.kolme.read().get_next_height(),
            peer: self.local_peer_id,
            timestamp: jiff::Timestamp::now(),
        })
        .publish(self, swarm)
        .await
        {
            tracing::error!("Unable to broadcast latest block height: {e:?}")
        }
    }

    async fn broadcast_mempool_entries(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        for tx in self.kolme.get_mempool_entries() {
            let txhash = tx.hash();
            let msg = GossipMessage::BroadcastTx { tx };
            if let Err(e) = msg.publish(self, swarm).await {
                tracing::error!("Unable to broadcast transaction {txhash}: {e:?}")
            }
        }
    }

    async fn handle_notification(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        notification: Result<Notification<App::Message>, RecvError>,
    ) {
        let notification = match notification {
            Ok(notification) => notification,
            Err(e) => {
                tracing::warn!("Gossip::handle_notification: received an error: {e}");
                return;
            }
        };
        if let Err(e) = GossipMessage::Notification(notification)
            .publish(self, swarm)
            .await
        {
            tracing::warn!("Error when handling notification: {e}");
        }
    }

    async fn handle_event(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        event: SwarmEvent<KolmeBehaviourEvent<App::Message>>,
        peers_with_blocks: &tokio::sync::mpsc::Sender<ReportBlockHeight>,
    ) {
        match event {
            SwarmEvent::ConnectionEstablished { .. } => {
                self.watch_network_ready.send_if_modified(|value| {
                    let ret = *value;
                    *value = true;
                    ret
                });
            }
            SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } => {
                tracing::info!(
                    "New listener {listener_id} on {address}, {:?}",
                    swarm.local_peer_id()
                );
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer, address) in peers {
                    tracing::info!("Discovered new peer over mDNS: {peer} @ {address}");
                    // TODO do we need to manually add mDNS peers to Kademlia?
                    swarm.behaviour_mut().kademlia.add_address(&peer, address);
                }
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            })) => {
                tracing::debug!("Received a message {message_id} from {propagation_source}");
                match GossipMessage::parse(self, message) {
                    Err(e) => {
                        tracing::warn!("Received a gossipsub message we couldn't parse: {e}");
                    }
                    Ok(message) => {
                        tracing::debug!("Received message: {message}");
                        if let Err(e) = self.handle_message(message, peers_with_blocks).await {
                            tracing::warn!("Error while handling message: {e}");
                        }
                    }
                }
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::RequestResponse(
                libp2p::request_response::Event::Message {
                    peer: _,
                    connection_id: _,
                    message,
                },
            )) => match message {
                libp2p::request_response::Message::Request {
                    request_id: _,
                    request,
                    channel,
                } => self.handle_request(request, channel, swarm).await,

                libp2p::request_response::Message::Response {
                    request_id: _,
                    response,
                } => self.handle_response(response).await,
            },
            _ => tracing::debug!("Received and ignoring libp2p event: {event:?}"),
        }
    }

    async fn handle_message(
        &self,
        message: GossipMessage<App>,
        peers_with_blocks: &tokio::sync::mpsc::Sender<ReportBlockHeight>,
    ) -> Result<()> {
        match message {
            GossipMessage::Notification(msg) => {
                tracing::debug!("got notification message");
                match &msg {
                    Notification::NewBlock(block) => {
                        self.add_block(block.clone()).await;
                    }
                    Notification::GenesisInstantiation { .. } => (),
                    // TODO should we validate that this has a proper signature from
                    // the processor before accepting it?
                    //
                    // See propose_and_await_transaction for an example.
                    Notification::FailedTransaction(_) => (),
                }
                self.kolme.notify(msg);
            }
            GossipMessage::RequestBlockHeights(_) => {
                tracing::debug!("got block heights request message");
                self.trigger_broadcast_height.send_modify(|old| *old += 1);
            }
            GossipMessage::ReportBlockHeight(report) => {
                tracing::debug!("got block height report message");
                let our_next = self.kolme.read().get_next_height();
                tracing::debug!("Received ReportBlockHeight: {report:?}, our_next: {our_next}");
                // Check if this peer has new blocks that we'd want to request.
                if our_next < report.next {
                    peers_with_blocks.try_send(report).ok();
                }
            }
            GossipMessage::BroadcastTx { tx } => {
                self.kolme.propose_transaction(tx);
            }
        }

        Ok(())
    }

    async fn handle_request(
        &self,
        request: BlockRequest,
        channel: ResponseChannel<BlockResponse<App::Message>>,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
    ) {
        match request {
            BlockRequest::BlockAtHeight(height) => {
                let res = match self.kolme.read().get_block(height).await {
                    Err(e) => {
                        tracing::warn!("Error querying block in gossip: {e}");
                        return;
                    }
                    Ok(None) => BlockResponse::HeightNotFound(height),
                    Ok(Some(storable_block)) => BlockResponse::Block(storable_block.block),
                };
                if let Err(e) = swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, res)
                {
                    tracing::warn!("Unable to answer BlockAtHeight request: {e:?}");
                }
            }
            BlockRequest::BlockWithStateAtHeight(height) => {
                let res = match self.kolme.read().get_block(height).await {
                    Err(e) => {
                        tracing::warn!("Error querying block (with state) in gossip: {e}");
                        return;
                    }
                    Ok(None) => BlockResponse::HeightNotFound(height),
                    Ok(Some(storable_block)) => {
                        let manager = self.kolme.get_merkle_manager();
                        let framework_state = manager.serialize(&storable_block.framework_state);
                        let app_state = manager.serialize(&storable_block.app_state);
                        match (framework_state, app_state) {
                            (Ok(framework_state), Ok(app_state)) => BlockResponse::BlockWithState {
                                block: storable_block.block,
                                framework_state,
                                app_state,
                                // FIXME should we store a hash of the logs in the block as well?
                                logs: storable_block.logs,
                            },
                            (Err(e), _) => {
                                tracing::warn!("Unable to serialize framework state: {e}");
                                return;
                            }
                            (_, Err(e)) => {
                                tracing::warn!("Unable to serialize app state: {e}");
                                return;
                            }
                        }
                    }
                };
                if let Err(e) = swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, res)
                {
                    tracing::warn!("Unable to answer BlockWithStateAtHeight request: {e:?}");
                }
            }
        }
    }

    async fn handle_response(&self, response: BlockResponse<App::Message>) {
        tracing::debug!("response");
        match response {
            BlockResponse::Block(block) => {
                self.add_block(block).await;
            }
            BlockResponse::BlockWithState {
                block,
                framework_state,
                app_state,
                logs,
            } => {
                if let Err(e) = self
                    .kolme
                    .add_block_with_state(block, framework_state, app_state, logs)
                    .await
                {
                    tracing::warn!("Unable to add block (with state) to chain: {e}");
                }
            }
            BlockResponse::HeightNotFound(height) => {
                tracing::warn!("Tried to find block height {height}, but peer didn't find it");
            }
        }
    }

    /// Catch up on the latest chain information, if needed.
    ///
    /// Both puts out a broadcast to find more peers, plus sends requests to those peers for latest height information and any missing blocks.
    async fn catch_up(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        report_block_height: Option<ReportBlockHeight>,
    ) {
        let ReportBlockHeight {
            next: their_next,
            peer,
            timestamp: _,
        } = match report_block_height {
            Some(report) => report,
            None => return,
        };

        let our_next = self.kolme.read().get_next_height();

        tracing::debug!(
            "In catch_up, their_node=={their_next}, peer=={peer}, our_next=={our_next}"
        );

        let their_highest = match their_next.prev() {
            None => return,
            Some(highest) => highest,
        };

        if their_highest < our_next {
            // They don't have any new blocks for us.
            return;
        }

        let do_state = match self.sync_mode {
            // Only do a state transfer if we've fallen more than 1 block behind.
            SyncMode::StateTransfer => their_highest != our_next,
            SyncMode::StateTransferForUpgrade => {
                todo!("Holding off on StateTransferForUpgrade until we handle upgrades")
            }
            SyncMode::BlockTransfer => false,
        };

        if do_state {
            swarm
                .behaviour_mut()
                .request_response
                .send_request(&peer, BlockRequest::BlockWithStateAtHeight(their_highest));
        } else {
            swarm
                .behaviour_mut()
                .request_response
                .send_request(&peer, BlockRequest::BlockAtHeight(our_next));
        }
    }

    async fn add_block(&self, block: Arc<SignedBlock<App::Message>>) {
        if block.0.message.as_inner().height == self.kolme.read().get_next_height() {
            if let Err(e) = self.kolme.add_block(block).await {
                tracing::warn!("Unable to add block to chain: {e}")
            }
        }
    }
}

/// Information on a bootstrap node to connect to over Kademlia.
///
/// This provides a [FromStr] impl that follows the format
/// `PEER_ID@MULTIADDR`
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct KademliaBootstrap {
    #[serde(
        serialize_with = "serialize_peer_id",
        deserialize_with = "deserialize_peer_id"
    )]
    pub peer: libp2p::PeerId,
    pub address: libp2p::Multiaddr,
}

impl FromStr for KademliaBootstrap {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let (peer, address) = s
            .split_once('@')
            .with_context(|| format!("No @ found in Kademlia bootstrap: {s}"))?;
        Ok(KademliaBootstrap {
            peer: peer.parse()?,
            address: address.parse()?,
        })
    }
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
