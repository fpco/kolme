use std::{
    collections::VecDeque,
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

use crate::*;

use libp2p::{
    futures::StreamExt,
    gossipsub::{self, IdentTopic},
    identity::Keypair,
    mdns, noise,
    request_response::ProtocolSupport,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use tokio::sync::Mutex;

/// A component that retrieves notifications from the network and broadcasts our own notifications back out.
pub struct Gossip<App: KolmeApp> {
    kolme: Kolme<App>,
    last_seen_watch: tokio::sync::watch::Sender<Option<BlockHeight>>,
    swarm: Mutex<Swarm<KolmeBehaviour<App::Message>>>,
    notifications: IdentTopic,
    find_peers: IdentTopic,
}

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
struct KolmeBehaviour<AppMessage: serde::de::DeserializeOwned + Send + 'static> {
    gossipsub: gossipsub::Behaviour,
    request_response:
        libp2p::request_response::cbor::Behaviour<BlockRequest, BlockResponse<AppMessage>>,
    mdns: mdns::tokio::Behaviour,
    kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
enum BlockRequest {
    /// Return the height of the next block to be generated.
    NextHeight,
    /// Return the contents of a specific block.
    BlockAtHeight(BlockHeight),
}

#[allow(clippy::large_enum_variant)]
#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(bound(
    serialize = "",
    deserialize = "AppMessage: serde::de::DeserializeOwned"
))]
enum BlockResponse<AppMessage: serde::de::DeserializeOwned> {
    Next(BlockHeight),
    Block(SignedBlock<AppMessage>),
    HeightNotFound(BlockHeight),
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
        let notifications = gossipsub::IdentTopic::new("/notifications/1.0");
        let find_peers = gossipsub::IdentTopic::new("/find-peers/1.0");
        // subscribes to our topics
        swarm.behaviour_mut().gossipsub.subscribe(&notifications)?;
        swarm.behaviour_mut().gossipsub.subscribe(&find_peers)?;

        // Begin listening based on the config
        fn add_listen<AppMessage: serde::de::DeserializeOwned + Send>(
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

        let (last_seen_watch, _) = tokio::sync::watch::channel(None);

        Ok(Gossip {
            kolme,
            last_seen_watch,
            swarm: Mutex::new(swarm),
            notifications,
            find_peers,
        })
    }
}

impl<App: KolmeApp> Gossip<App> {
    pub fn subscribe_last_seen(&self) -> tokio::sync::watch::Receiver<Option<BlockHeight>> {
        self.last_seen_watch.subscribe()
    }

    pub async fn run(self) -> Result<()> {
        let mut subscription = self.kolme.subscribe();
        let mut swarm = self.swarm.lock().await;
        let mut event_state = EventState::default();

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                notification = subscription.recv() =>
                    self.handle_notification(&mut swarm, notification? ).await?,
                event = swarm.select_next_some() => self.handle_event(&mut swarm, event, &mut event_state).await?,
                _ = interval.tick() => self.catch_up(&mut swarm, &mut event_state).await,
            }
        }
    }

    async fn handle_notification(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        notification: Notification<App::Message>,
    ) -> Result<()> {
        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(
            self.notifications.clone(),
            serde_json::to_vec(&notification)?,
        ) {
            tracing::debug!("Error when handling notification: {e}");
        }
        Ok(())
    }

    async fn handle_event(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        event: SwarmEvent<KolmeBehaviourEvent<App::Message>>,
        state: &mut EventState,
    ) -> Result<()> {
        match event {
            SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } => {
                tracing::debug!("New listener {listener_id} on {address}");
                let peer = *swarm.local_peer_id();
                swarm.behaviour_mut().kademlia.add_address(&peer, address);
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer, address) in peers {
                    state.add_peer(peer);
                    swarm.behaviour_mut().kademlia.add_address(&peer, address);
                }
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Kademlia(event)) => {
                match event {
                    libp2p::kad::Event::OutboundQueryProgressed { id: _, result, .. } => {
                        // Handle query results (e.g., found peers)
                        if let libp2p::kad::QueryResult::GetClosestPeers(Ok(peers)) = result {
                            for peer in peers.peers {
                                state.add_peer(peer.peer_id);
                            }
                        }
                    }
                    libp2p::kad::Event::RoutablePeer { peer, address } => {
                        tracing::debug!("Routable peer: {} at {}", peer, address);
                        state.add_peer(peer);
                        swarm.dial(peer).ok();
                    }
                    _ => tracing::debug!("Kademlia event: {:?}", event),
                }
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            })) => {
                if message.topic == self.notifications.hash() {
                    tracing::debug!("Received a message {message_id} from {propagation_source}");
                    match serde_json::from_slice::<Notification<App::Message>>(&message.data) {
                        Ok(msg) => {
                            match &msg {
                                Notification::NewBlock(block) => {
                                    state.observe_next_block_height(
                                        block.0.message.as_inner().height.next(),
                                    );
                                    self.add_block(block.clone()).await;
                                }
                                Notification::GenesisInstantiation { .. } => (),
                                Notification::Broadcast { .. } => (),
                                Notification::FailedTransaction { .. } => (),
                            }
                            self.kolme.notify(msg);
                        }
                        Err(e) => {
                            tracing::warn!("Unable to parse content from {message_id}, sent by {propagation_source}: {e}")
                        }
                    }
                } else if message.topic == self.find_peers.hash() {
                    if message.data == FIND_PEER_REQUEST {
                        swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(self.find_peers.clone(), FIND_PEER_RESPONSE)?;
                    } else if message.data == FIND_PEER_RESPONSE {
                        // FIXME send these back via request/response instead
                        state.add_peer(propagation_source);
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
                } => match request {
                    BlockRequest::NextHeight => {
                        if let Err(e) = swarm.behaviour_mut().request_response.send_response(
                            channel,
                            BlockResponse::Next(self.kolme.read().get_next_height()),
                        ) {
                            tracing::warn!("Unable to answer Next request: {e:?}");
                        }
                    }
                    BlockRequest::BlockAtHeight(height) => {
                        let res = match self.kolme.read().get_block(height).await? {
                            None => BlockResponse::HeightNotFound(height),
                            Some(storable_block) => {
                                BlockResponse::Block(Arc::unwrap_or_clone(storable_block.block))
                            }
                        };
                        if let Err(e) = swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(channel, res)
                        {
                            tracing::warn!("Unable to answer GetHeight request: {e:?}");
                        }
                    }
                },
                libp2p::request_response::Message::Response {
                    request_id: _,
                    response,
                } => match response {
                    BlockResponse::Next(next_height) => {
                        let updated = state.observe_next_block_height(next_height);
                        // event state starts with BlockHeight::start (equal to 0) and if max next height
                        // was updated we should have received at least 1 but we do an extra check for safety
                        if updated && !next_height.is_start() {
                            let height = BlockHeight(next_height.0 - 1);
                            if let Err(e) = self.last_seen_watch.send(Some(height)) {
                                tracing::warn!(
                                    "Unable to notify about new last seen block height: {e}"
                                )
                            }
                        }
                    }
                    BlockResponse::Block(block) => {
                        self.add_block(Arc::new(block)).await;
                    }
                    BlockResponse::HeightNotFound(height) => {
                        tracing::warn!(
                            "Tried to find block height {height}, but peer didn't find it"
                        );
                    }
                },
            },
            _ => tracing::debug!("Received and ignoring libp2p event: {event:?}"),
        }
        Ok(())
    }

    /// Catch up on the latest chain information, if needed.
    ///
    /// Both puts out a broadcast to find more peers, plus sends requests to those peers for latest height information and any missing blocks.
    async fn catch_up(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        state: &mut EventState,
    ) {
        if let Err(e) = swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.find_peers.clone(), FIND_PEER_REQUEST)
        {
            tracing::warn!("Error when trying to request peers: {e}")
        }

        // Kademlia: Start a query to find closest peers
        let peer_id = *swarm.local_peer_id();
        swarm.behaviour_mut().kademlia.get_closest_peers(peer_id);

        if let Some(peer) = state.get_next_peer() {
            swarm
                .behaviour_mut()
                .request_response
                .send_request(peer, BlockRequest::NextHeight);
        }

        let next = self.kolme.read().get_next_height();
        if state.expected_next_block > next {
            if let Some(peer) = state.get_next_peer() {
                swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(peer, BlockRequest::BlockAtHeight(next));
            }
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

const PEER_COUNT: usize = 16;
struct EventState {
    peers: VecDeque<PeerId>,
    expected_next_block: BlockHeight,
}

impl Default for EventState {
    fn default() -> Self {
        EventState {
            peers: VecDeque::with_capacity(PEER_COUNT),
            expected_next_block: BlockHeight::start(),
        }
    }
}

impl EventState {
    fn add_peer(&mut self, peer: PeerId) {
        if !self.peers.contains(&peer) {
            if self.peers.len() >= PEER_COUNT {
                assert!(self.peers.len() == PEER_COUNT);
                self.peers.pop_back();
            }
            self.peers.push_front(peer);
        }
        tracing::debug!("Current list of peers: {:?}", self.peers);
    }

    /// stores max next height and returns true if it was updated
    fn observe_next_block_height(&mut self, next: BlockHeight) -> bool {
        if next > self.expected_next_block {
            self.expected_next_block = next;
            true
        } else {
            false
        }
    }

    fn get_next_peer(&mut self) -> Option<&PeerId> {
        if self.peers.is_empty() {
            None
        } else {
            self.peers.rotate_left(1);
            self.peers.back()
        }
    }
}

const FIND_PEER_REQUEST: &[u8] = b"request";
const FIND_PEER_RESPONSE: &[u8] = b"response";
