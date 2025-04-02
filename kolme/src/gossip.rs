use std::{
    collections::VecDeque,
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

use crate::*;

use libp2p::{
    futures::StreamExt,
    gossipsub::{self, IdentTopic},
    mdns, noise,
    request_response::ProtocolSupport,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, PeerId, StreamProtocol, Swarm,
};
use tokio::sync::Mutex;

/// A component that retrieves notifications from the network and broadcasts our own notifications back out.
pub struct Gossip<App: KolmeApp> {
    kolme: Kolme<App>,
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
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
enum BlockRequest {
    /// Return the height of the next block to be generated.
    Next,
    /// Return the contents of a specific block.
    GetHeight(BlockHeight),
}

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

impl<App: KolmeApp> Gossip<App> {
    pub async fn new(kolme: Kolme<App>) -> Result<Self> {
        let mut swarm = libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            .with_behaviour(|key| {
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
                Ok(KolmeBehaviour {
                    gossipsub,
                    mdns,
                    request_response,
                })
            })?
            .build();

        // Create the Gossipsub topics
        let notifications = gossipsub::IdentTopic::new("/notifications/1.0");
        let find_peers = gossipsub::IdentTopic::new("/find-peers/1.0");
        // subscribes to our topics
        swarm.behaviour_mut().gossipsub.subscribe(&notifications)?;
        swarm.behaviour_mut().gossipsub.subscribe(&find_peers)?;

        // Listen on all interfaces and whatever port the OS assigns
        swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;
        swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        swarm.listen_on("/ip6/::/udp/0/quic-v1".parse()?)?;
        swarm.listen_on("/ip6/::/tcp/0".parse()?)?;

        Ok(Self {
            kolme,
            swarm: Mutex::new(swarm),
            notifications,
            find_peers,
        })
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
            } => tracing::info!("New listener {listener_id} on {address}"),
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer, _) in peers {
                    state.add_peer(peer);
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
                                    self.add_block(block).await;
                                }
                                Notification::GenesisInstantiation { .. } => (),
                                Notification::Broadcast { .. } => (),
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
                libp2p::request_response::Event::Message { peer: _, message },
            )) => match message {
                libp2p::request_response::Message::Request {
                    request_id: _,
                    request,
                    channel,
                } => match request {
                    BlockRequest::Next => {
                        if let Err(e) = swarm.behaviour_mut().request_response.send_response(
                            channel,
                            BlockResponse::Next(self.kolme.read().await.get_next_height()),
                        ) {
                            tracing::warn!("Unable to answer Next request: {e:?}");
                        }
                    }
                    BlockRequest::GetHeight(height) => {
                        let res = match self.kolme.read().await.get_block(height).await? {
                            None => BlockResponse::HeightNotFound(height),
                            Some(block) => BlockResponse::Block(block),
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
                    BlockResponse::Next(height) => {
                        state.observe_next_block_height(height);
                    }
                    BlockResponse::Block(block) => {
                        self.add_block(&block).await;
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

        if let Some(peer) = state.get_next_peer() {
            swarm
                .behaviour_mut()
                .request_response
                .send_request(peer, BlockRequest::Next);
        }

        let next = self.kolme.read().await.get_next_height();
        if state.expected_next_block > next {
            if let Some(peer) = state.get_next_peer() {
                swarm
                    .behaviour_mut()
                    .request_response
                    .send_request(peer, BlockRequest::GetHeight(next));
            }
        }
    }

    async fn add_block(&self, block: &SignedBlock<App::Message>) {
        if block.0.message.as_inner().height == self.kolme.read().await.get_next_height() {
            if let Err(e) = self.kolme.add_block(block.clone()).await {
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

    fn observe_next_block_height(&mut self, next: BlockHeight) {
        self.expected_next_block = self.expected_next_block.max(next);
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
