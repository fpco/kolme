mod messages;
mod sync_manager;

use std::{
    fmt::Display,
    hash::{DefaultHasher, Hash, Hasher},
    num::NonZeroUsize,
    str::FromStr,
    time::{Duration, Instant},
};

use crate::*;
use messages::*;

use libp2p::{
    autonat, dcutr,
    futures::StreamExt,
    gossipsub::{self, IdentTopic},
    identify,
    kad::RecordKey,
    noise, ping, relay, rendezvous,
    request_response::{ProtocolSupport, ResponseChannel},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, upnp, yamux, StreamProtocol, Swarm, SwarmBuilder,
};
use sync_manager::{DataLabel, DataRequest, SyncManager};
use tokio::sync::{broadcast::error::RecvError, Mutex};

pub use libp2p::{identity::Keypair, Multiaddr, PeerId};
use utils::trigger::Trigger;

/// A component that retrieves notifications from the network and broadcasts our own notifications back out.
pub struct Gossip<App: KolmeApp> {
    kolme: Kolme<App>,
    swarm: Mutex<Swarm<KolmeBehaviour<App::Message>>>,
    gossip_topic: IdentTopic,
    sync_mode: SyncMode,
    data_load_validation: DataLoadValidation,
    local_peer_id: PeerId,
    // human-readable name for an instance
    local_display_name: String,
    /// Trigger a broadcast of our latest block height.
    trigger_broadcast_height: Trigger,
    /// Switches to true once we have our first success received message
    watch_network_ready: tokio::sync::watch::Sender<bool>,
    /// The block sync manager.
    sync_manager: Mutex<SyncManager<App>>,
    /// LRU cache for notifications received via P2p layer
    lru_notifications: parking_lot::RwLock<lru::LruCache<Sha256Hash, Instant>>,
    /// Kademlia key for discovering other peers for this network.
    ///
    /// MSS 2025-07-24: I've found conflicting information about Kademlia peer discovery
    /// online. Some of it indicates that bootstrap for peer discovery should be fully
    /// automatic. Other sources say that we need to get providers of a shared key to
    /// find nodes. Empirically, adding in this start_providing/get_providers bit seems
    /// to have improved the discovery process, so including, but this is worth deeper
    /// investigation in the future.
    dht_key: RecordKey,
    concurrent_request_limit: usize,
    max_peer_count: usize,
    warning_period: Duration,
}

// We create a custom network behaviour that combines Gossipsub, Request/Response and Kademlia.
#[derive(NetworkBehaviour)]
struct KolmeBehaviour<AppMessage: serde::de::DeserializeOwned + Send + Sync + 'static> {
    gossipsub: gossipsub::Behaviour,
    request_response:
        libp2p::request_response::cbor::Behaviour<BlockRequest, BlockResponse<AppMessage>>,
    kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    autonat: autonat::Behaviour,
    dcutr: dcutr::Behaviour,
    upnp: upnp::tokio::Behaviour,
    relay: relay::Behaviour,
    rendezvous: rendezvous::client::Behaviour,
}

/// Config for a Gossip listener.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GossipListener {
    pub proto: GossipProto,
    pub ip: GossipIp,
    /// Use 0 to grab an available port.
    pub port: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GossipProto {
    Tcp,
    Quic,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GossipIp {
    Ip4,
    Ip6,
}

impl GossipListener {
    /// Generates a random listener with an available port.
    pub fn random() -> Result<Self> {
        let port = std::net::TcpListener::bind("0.0.0.0:0")?
            .local_addr()?
            .port();
        Ok(Self {
            proto: GossipProto::Tcp,
            ip: GossipIp::Ip4,
            port,
        })
    }

    /// Produce the Multiaddr for this listener.
    pub fn multiaddr(&self) -> Multiaddr {
        match format!(
            "{}/{}/{}{}",
            match self.ip {
                GossipIp::Ip4 => "/ip4/0.0.0.0",
                GossipIp::Ip6 => "/ip6/::",
            },
            match self.proto {
                GossipProto::Tcp => "tcp",
                GossipProto::Quic => "udp",
            },
            self.port,
            match self.proto {
                GossipProto::Tcp => "",
                GossipProto::Quic => "/quic-v1",
            }
        )
        .parse()
        {
            Ok(addr) => addr,
            Err(e) => panic!("GossipListener::multiaddr failed on {self:?}: {e}"),
        }
    }
}

pub struct GossipBuilder {
    keypair: Option<Keypair>,
    bootstrap: Vec<(PeerId, Multiaddr)>,
    listeners: Vec<GossipListener>,
    /// See [libp2p::gossipsub::Configbuilder::heartbeat_interval]
    heartbeat_interval: Duration,
    sync_mode: SyncMode,
    data_load_validation: DataLoadValidation,
    local_display_name: Option<String>,
    duplicate_cache_time: Duration,
    external_addrs: Vec<Multiaddr>,
    concurrent_request_limit: usize,
    max_peer_count: usize,
    warning_period: Duration,
}

impl Default for GossipBuilder {
    fn default() -> Self {
        Self {
            keypair: Default::default(),
            bootstrap: Default::default(),
            listeners: vec![],
            // This is set to aid debugging by not cluttering the log space
            heartbeat_interval: Duration::from_secs(10),
            sync_mode: Default::default(),
            data_load_validation: Default::default(),
            local_display_name: Default::default(),
            // Same default as libp2p_gossip
            duplicate_cache_time: Duration::from_secs(60),
            external_addrs: vec![],
            concurrent_request_limit: sync_manager::DEFAULT_REQUEST_COUNT,
            max_peer_count: sync_manager::DEFAULT_PEER_COUNT,
            warning_period: Duration::from_secs(sync_manager::DEFAULT_WARNING_PERIOD_SECS),
        }
    }
}

/// How block data is synchronized.
///
/// Default: [SyncMode::StateTransfer]
#[derive(Default, Debug)]
pub enum SyncMode {
    /// Allow state transfer always (aka fast sync).
    ///
    /// Requires trust in the processor to only produce valid blocks, no verification occurs on our node.
    ///
    /// Will still use block sync (executing a block locally) when syncing the newest block,
    /// while this node is up-to-date, and if the node is running the right code version.
    ///
    /// Note that this will attempt to sync to the latest block. See [SyncMode::Archive] if you
    /// want all the blocks.
    #[default]
    StateTransfer,
    /// Same as [SyncMode::StateTransfer], but syncs all blocks from the beginning of the chain.
    /// Allow state transfer for version upgrades, but otherwise use block sync.
    Archive,
    /// Always do block sync, verifying each new block.
    ///
    /// This will fail to work if the chain has different code versions. At the point of a version
    /// upgrade, you would need to switch to a new code version.
    BlockTransfer,
}

impl GossipBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_bootstrap(mut self, peer: PeerId, address: Multiaddr) -> Self {
        self.bootstrap.push((peer, address));
        self
    }

    pub fn set_duplicate_cache_time(mut self, cache_time: Duration) -> Self {
        self.duplicate_cache_time = cache_time;
        self
    }

    pub fn set_keypair(mut self, keypair: Keypair) -> Self {
        self.keypair = Some(keypair);
        self
    }

    /// Add a listener
    ///
    /// If none are provided, one random listener is set up.
    pub fn add_listener(mut self, listener: GossipListener) -> Self {
        self.listeners.push(listener);
        self
    }

    /// Add an external address.
    ///
    /// For situations like NAT traversals and load balancers, this allows
    /// the node to advertise its publicly accessible address.
    pub fn add_external_address(mut self, addr: Multiaddr) -> Self {
        self.external_addrs.push(addr);
        self
    }

    /// Set time between each heartbeat (default is 10 seconds).
    pub fn heartbeat_interval(mut self, heartbeat_interval: Duration) -> Self {
        self.heartbeat_interval = heartbeat_interval;
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

    pub fn set_local_display_name(mut self, display_name: &str) -> Self {
        self.local_display_name = Some(String::from(display_name));
        self
    }

    /// Set the number of allowed concurrent data requests when state syncing.
    pub fn set_concurrent_request_limit(mut self, limit: usize) -> Self {
        self.concurrent_request_limit = limit;
        self
    }

    /// Set the maximum number of peers to query simultaneously for data.
    pub fn set_max_peer_count(mut self, count: usize) -> Self {
        self.max_peer_count = count;
        self
    }

    /// Set the duration to wait before printing a warning about block/layer data not being downloaded.
    pub fn set_data_warning_period(mut self, period: Duration) -> Self {
        self.warning_period = period;
        self
    }

    pub fn build<App: KolmeApp>(self, kolme: Kolme<App>) -> Result<Gossip<App>> {
        let builder = match self.keypair {
            Some(keypair) => SwarmBuilder::with_existing_identity(keypair),
            None => SwarmBuilder::with_new_identity(),
        };
        let mut swarm = builder
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_quic()
            .with_dns()?
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
                    .duplicate_cache_time(self.duplicate_cache_time)
                    .heartbeat_interval(self.heartbeat_interval)
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

                let identify = identify::Behaviour::new(identify::Config::new(
                    "/kolme/1.0.0".to_owned(),
                    key.public(),
                ));
                let ping = ping::Behaviour::new(ping::Config::new());
                let autonat =
                    autonat::Behaviour::new(key.public().to_peer_id(), autonat::Config::default());
                let dcutr = dcutr::Behaviour::new(key.public().to_peer_id());
                let upnp = upnp::tokio::Behaviour::default();
                let relay =
                    relay::Behaviour::new(key.public().to_peer_id(), relay::Config::default());
                let rendezvous = rendezvous::client::Behaviour::new(key.clone());

                Ok(KolmeBehaviour {
                    gossipsub,
                    request_response,
                    kademlia,
                    identify,
                    ping,
                    autonat,
                    dcutr,
                    upnp,
                    relay,
                    rendezvous,
                })
            })?
            .build();

        // Create the Gossipsub topics
        let full_genesis_hash = kolme.get_genesis_hash()?;
        let genesis_hash = FirstEightChars(full_genesis_hash);
        let gossip_topic = gossipsub::IdentTopic::new(format!("/kolme-gossip/{genesis_hash}/1.0"));
        tracing::info!("Subscribing to gossip. Full genesis hash: {full_genesis_hash}. Abbreviated: {genesis_hash}. Topic: {gossip_topic}.");
        // And subscribe
        swarm.behaviour_mut().gossipsub.subscribe(&gossip_topic)?;

        if self.listeners.is_empty() {
            swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        } else {
            for listener in &self.listeners {
                swarm.listen_on(listener.multiaddr())?;
            }
        }

        for addr in self.external_addrs {
            swarm.add_external_address(addr);
        }

        let (watch_network_ready, _) = tokio::sync::watch::channel(false);
        let local_peer_id = *swarm.local_peer_id();
        let sync_manager = Mutex::new(SyncManager::default());
        let dht_key = RecordKey::new(&format!("/kolme-dht/{genesis_hash}/1.0"));

        Ok(Gossip {
            kolme,
            swarm: Mutex::new(swarm),
            gossip_topic,
            sync_mode: self.sync_mode,
            data_load_validation: self.data_load_validation,
            local_peer_id,
            trigger_broadcast_height: Trigger::new("broadcast_height"),
            watch_network_ready,
            local_display_name: self.local_display_name.unwrap_or(String::from("gossip")),
            sync_manager,
            lru_notifications: lru::LruCache::new(NonZeroUsize::new(100).unwrap()).into(),
            dht_key,
            concurrent_request_limit: self.concurrent_request_limit,
            max_peer_count: self.max_peer_count,
            warning_period: self.warning_period,
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

        // Interval for broadcasting our block height
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.reset_immediately();
        let mut trigger_broadcast_height = self.trigger_broadcast_height.subscribe();

        let mut mempool_additions = self.kolme.subscribe_mempool_additions();

        let (block_requester, mut block_requester_rx) = tokio::sync::mpsc::channel(8);
        self.kolme.set_block_requester(block_requester);

        let mut sync_manager_subscriber = self.sync_manager.lock().await.subscribe();

        self.dht_peer_discovery(&mut swarm);

        loop {
            tokio::select! {
                // Our local Kolme generated a notification to be sent through the
                // rest of the p2p network
                notification = subscription.recv() => {
                    self.handle_notification(&mut swarm, notification);
                }
                // A new event was generated from the p2p network
                event = swarm.select_next_some() => {
                    self.handle_event(&mut swarm, event).await;
                }
                // Periodically notify the p2p network of our latest block height
                // Also use this time to broadcast any transactions from the mempool.
                _ = interval.tick() => {
                    self.on_interval(&mut swarm).await;
                },
                _ = sync_manager_subscriber.listen() => {
                    self.process_sync_manager(&mut swarm).await;
                }
                // When we're specifically triggered for it, also notify for latest block height
                _ = trigger_broadcast_height.listen() => {
                    self.broadcast_latest_block(&mut swarm);
                }
                // And any time we add something new to the mempool, broadcast all items.
                _ = mempool_additions.listen() => {
                    self.broadcast_mempool_entries(&mut swarm);
                }
                Some(height) = block_requester_rx.recv() => {
                    if let Err(e) = self.sync_manager.lock().await.add_needed_block(&self, height, None).await {
                        tracing::warn!("{}: error when adding requested block {height}: {e}", self.local_display_name)
                    }
                }
            }
        }
    }

    fn dht_peer_discovery(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        // Help out peer discovery, this is _probably_ not appropriate to run
        // every 5 seconds, but we'll start here.
        if let Err(e) = swarm
            .behaviour_mut()
            .kademlia
            .start_providing(self.dht_key.clone())
        {
            tracing::error!(
                "{}: unable to start providing DHT key {:?}: {e}",
                self.local_display_name,
                self.dht_key
            );
        }
        swarm
            .behaviour_mut()
            .kademlia
            .get_providers(self.dht_key.clone());
    }

    async fn on_interval(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        self.request_block_heights(swarm);
        self.process_sync_manager(swarm).await;
        self.broadcast_latest_block(swarm);
        self.broadcast_mempool_entries(swarm);
        self.dht_peer_discovery(swarm);
    }

    fn request_block_heights(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        if *self.watch_network_ready.borrow() {
            // We already sent this request successfully, no need to repeat.
            return;
        }
        match GossipMessage::RequestBlockHeights(jiff::Timestamp::now()).publish(self, swarm) {
            Ok(sent) => {
                if sent {
                    tracing::info!(
                        "{}: Successfully sent a block height request, p2p network is ready",
                        self.local_display_name
                    );
                    self.watch_network_ready.send_replace(true);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "{}: Unable to request block heights: {e:?}",
                    self.local_display_name
                );
            }
        }
    }

    fn broadcast_latest_block(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        if let Err(e) = GossipMessage::ReportBlockHeight(ReportBlockHeight {
            next: self.kolme.read().get_next_height(),
            peer: self.local_peer_id,
            timestamp: jiff::Timestamp::now(),
            latest_block: self.kolme.get_latest_block(),
        })
        .publish(self, swarm)
        {
            tracing::error!(
                "{}: Unable to broadcast latest block height: {e:?}",
                self.local_display_name
            )
        }
    }

    fn broadcast_mempool_entries(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        for tx in self.kolme.get_mempool_entries_for_gossip() {
            let txhash = tx.hash();
            let msg = GossipMessage::BroadcastTx { tx };
            match msg.publish(self, swarm) {
                Ok(true) => self.kolme.mark_mempool_entry_gossiped(txhash),
                // No peers received the message
                Ok(false) => (),
                Err(e) => tracing::error!(
                    "{}: Unable to broadcast transaction {txhash}: {e:?}",
                    self.local_display_name
                ),
            }
        }
    }

    fn handle_notification(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        notification: Result<Notification<App::Message>, RecvError>,
    ) {
        let notification = match notification {
            Ok(notification) => notification,
            Err(e) => {
                tracing::warn!(
                    "{}: Gossip::handle_notification: received an error: {e}",
                    self.local_display_name
                );
                return;
            }
        };

        if let Some(hash) = notification.hash() {
            if self.notification_hash_exists(&hash) {
                tracing::debug!("Skipping publish to p2p layer");
                return;
            }
        }
        if let Err(e) = GossipMessage::Notification(notification).publish(self, swarm) {
            tracing::warn!(
                "{}: Error when handling notification: {e}",
                self.local_display_name
            );
        }
    }

    async fn handle_event(
        &self,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
        event: SwarmEvent<KolmeBehaviourEvent<App::Message>>,
    ) {
        let local_display_name = self.local_display_name.clone();

        match event {
            SwarmEvent::NewListenAddr {
                listener_id,
                address,
            } => {
                tracing::info!(
                    "{local_display_name}: New listener {listener_id} on {address}, {:?}",
                    swarm.local_peer_id()
                );
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            })) => {
                tracing::debug!(
                    "{local_display_name}: Received a message {message_id} from {propagation_source}"
                );
                match GossipMessage::parse(self, message) {
                    Err(e) => {
                        tracing::warn!(
                            "{local_display_name}: Received a gossipsub message we couldn't parse: {e}"
                        );
                    }
                    Ok(message) => {
                        tracing::debug!("{local_display_name}: Received message: {message}");
                        self.handle_message(message, swarm).await;
                    }
                }
            }
            SwarmEvent::Behaviour(KolmeBehaviourEvent::RequestResponse(
                libp2p::request_response::Event::Message {
                    peer,
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
                } => self.handle_response(response, peer).await,
            },
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id: _,
                endpoint,
                num_established: _,
                concurrent_dial_errors: _,
                established_in,
            } =>
                tracing::info!("{local_display_name}: connection established to {peer_id} on {endpoint:?}, established in {established_in:?}"),
            // Catch some events that we don't even want to print at debug level
            SwarmEvent::Behaviour(KolmeBehaviourEvent::RequestResponse(
                libp2p::request_response::Event::ResponseSent { .. },
            ))
            | SwarmEvent::Behaviour(KolmeBehaviourEvent::Kademlia(
                libp2p::kad::Event::OutboundQueryProgressed { .. },
            )) => tracing::trace!(
                "{local_display_name}: Received and ignoring libp2p event: {event:?}"
            ),
            _ => tracing::debug!(
                "{local_display_name}: Received and ignoring libp2p event: {event:?}"
            ),
        }
    }

    async fn handle_message(
        &self,
        message: GossipMessage<App>,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
    ) {
        let local_display_name = self.local_display_name.clone();
        match message {
            GossipMessage::Notification(msg) => {
                tracing::debug!("{local_display_name}: got notification message");
                if let Some(hash) = &msg.hash() {
                    self.check_and_update_notification_lru(hash);
                }
                match &msg {
                    Notification::NewBlock(block) => {
                        self.sync_manager
                            .lock()
                            .await
                            .add_new_block(self, block)
                            .await;

                        if let Err(e) = self.kolme.resync().await {
                            tracing::error!(%self.local_display_name, "Error while resyncing after a NewBlock notification: {e}");
                        }
                    }
                    Notification::GenesisInstantiation { .. } => (),
                    // TODO should we validate that this has a proper signature from
                    // the processor before accepting it?
                    //
                    // See propose_and_await_transaction for an example.
                    Notification::FailedTransaction(_) => (),
                    Notification::LatestBlock(_) => {
                        if let Err(e) = self.kolme.resync().await {
                            tracing::error!(%self.local_display_name, "Error while resyncing after a LatestBlock notification: {e}");
                        }
                    }
                    Notification::EvictMempoolTransaction(signed_json) => {
                        let pubkey = signed_json.verify_signature();
                        match pubkey {
                            Ok(pubkey) => {
                                let processor = self
                                    .kolme
                                    .read()
                                    .get_framework_state()
                                    .get_validator_set()
                                    .processor;
                                if pubkey != processor {
                                    tracing::warn!("Evict transaction was signed by {pubkey}, but processor is {processor}");
                                } else {
                                    let txhash = signed_json.message.as_inner();
                                    tracing::debug!("Transaction {txhash} evicted from mempool");
                                    self.kolme.remove_from_mempool(*txhash);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Error verifying evict mempool signature: {e}")
                            }
                        }
                    }
                }
                self.kolme.notify(msg);
            }
            GossipMessage::RequestBlockHeights(_) => {
                tracing::debug!("{local_display_name}: got block heights request message");
                self.trigger_broadcast_height.trigger();
            }
            GossipMessage::ReportBlockHeight(report) => {
                self.sync_manager
                    .lock()
                    .await
                    .add_report_block_height(self, report)
                    .await;
            }
            GossipMessage::BroadcastTx { tx } => {
                let txhash = tx.hash();
                self.kolme.propose_transaction(tx);
                self.kolme.mark_mempool_entry_gossiped(txhash);
            }
            GossipMessage::RequestBlockContents { height, peer } => {
                match self.kolme.has_block(height).await {
                    Err(e) => {
                        tracing::warn!(
                            "{local_display_name}: RequestBlockContents error on {height}: {e}"
                        );
                    }
                    Ok(false) => (),
                    Ok(true) => {
                        swarm.behaviour_mut().request_response.send_request(
                            &peer,
                            BlockRequest::BlockAvailable {
                                height,
                                peer: self.peer_id(),
                            },
                        );
                    }
                }
            }
            GossipMessage::RequestLayerContents { hash, peer } => {
                match self.kolme.has_merkle_hash(hash).await {
                    Err(e) => tracing::warn!(
                        "{local_display_name}: RequestLayerContents error on {hash}: {e}"
                    ),
                    Ok(false) => (),
                    Ok(true) => {
                        swarm.behaviour_mut().request_response.send_request(
                            &peer,
                            BlockRequest::LayerAvailable {
                                hash,
                                peer: self.peer_id(),
                            },
                        );
                    }
                }
            }
        }
    }

    async fn handle_request(
        &self,
        request: BlockRequest,
        channel: ResponseChannel<BlockResponse<App::Message>>,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
    ) {
        let local_display_name = self.local_display_name.clone();
        let res = match &request {
            BlockRequest::BlockAtHeight(height) | BlockRequest::BlockWithStateAtHeight(height) => {
                let height = *height;
                match self.kolme.read().get_block(height).await {
                    Err(e) => {
                        tracing::warn!("{local_display_name}: Error querying block in gossip: {e}");
                        BlockResponse::HeightNotFound(height)
                    }
                    Ok(None) => {
                        tracing::warn!("{local_display_name}: Received a request for block {height}, but didn't have it.");
                        BlockResponse::HeightNotFound(height)
                    }
                    Ok(Some(storable_block)) => {
                        #[cfg(debug_assertions)]
                        {
                            // Sanity testing
                            let block = storable_block.block.0.message.as_inner();
                            for hash in [block.framework_state, block.app_state, block.logs] {
                                if self.kolme.get_merkle_layer(hash).await.unwrap().is_none() {
                                    panic!("{local_display_name}: has block with hash {hash}, but that hash isn't in the store: {block:?}");
                                }
                            }
                        }
                        BlockResponse::Block(storable_block.block)
                    }
                }
            }
            BlockRequest::Merkle(hash) => {
                let hash = *hash;
                match self.kolme.get_merkle_layer(hash).await {
                    // We didn't have it, in theory we could send a message back about this, but
                    // skipping for now
                    Ok(None) => {
                        tracing::warn!("{local_display_name}: Received a request for merkle layer {hash}, but didn't have it.");
                        BlockResponse::MerkleNotFound(hash)
                    }
                    Ok(Some(contents)) => BlockResponse::Merkle { hash, contents },
                    Err(e) => {
                        tracing::warn!(
                            "{local_display_name}: Error when loading Merkle layer for {hash}: {e}"
                        );
                        BlockResponse::MerkleNotFound(hash)
                    }
                }
            }
            BlockRequest::BlockAvailable { height, peer } => {
                self.sync_manager
                    .lock()
                    .await
                    .add_block_peer(self, *height, *peer);
                BlockResponse::Ack
            }
            BlockRequest::LayerAvailable { hash, peer } => {
                self.sync_manager
                    .lock()
                    .await
                    .add_layer_peer(self, *hash, *peer);
                BlockResponse::Ack
            }
        };

        if let Err(e) = swarm
            .behaviour_mut()
            .request_response
            .send_response(channel, res)
        {
            tracing::warn!("{local_display_name}: Unable to answer {request}: {e:?}");
        }
    }

    async fn handle_response(&self, response: BlockResponse<App::Message>, peer: PeerId) {
        let local_display_name = self.local_display_name.clone();
        match response {
            BlockResponse::Block(block) | BlockResponse::BlockWithState { block } => {
                self.sync_manager
                    .lock()
                    .await
                    .add_pending_block(self, block, Some(peer))
                    .await;
            }
            BlockResponse::HeightNotFound(height) => {
                tracing::warn!(
                    "{local_display_name}: Tried to find block height {height}, but peer {peer} didn't find it"
                );
                self.sync_manager
                    .lock()
                    .await
                    .remove_block_peer(height, peer);
            }
            BlockResponse::MerkleNotFound(hash) => {
                tracing::warn!(
                    "{local_display_name}: Tried to find merkle layer {hash}, but peer {peer} didn't find it"
                );
                self.sync_manager.lock().await.remove_layer_peer(hash, peer);
            }
            BlockResponse::Merkle { hash, contents } => {
                if let Err(e) = self
                    .sync_manager
                    .lock()
                    .await
                    .add_merkle_layer(self, hash, contents, peer)
                    .await
                {
                    tracing::error!(
                        "{local_display_name}: error adding Merkle layer contents for {hash}: {e}"
                    )
                }
            }
            BlockResponse::Ack => (),
        }
    }

    async fn process_sync_manager(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        let requests = match self.sync_manager.lock().await.get_data_requests(self).await {
            Ok(requests) => requests,
            Err(e) => {
                tracing::error!(
                    "{}: unable to process sync manager requests: {e}",
                    self.local_display_name
                );
                return;
            }
        };
        for DataRequest {
            data,
            current_peers,
            request_new_peers,
        } in requests
        {
            if request_new_peers || current_peers.is_empty() {
                let msg = match data {
                    DataLabel::Block(height) => GossipMessage::RequestBlockContents {
                        height,
                        peer: self.peer_id(),
                    },
                    DataLabel::Merkle(hash) => GossipMessage::RequestLayerContents {
                        hash,
                        peer: self.peer_id(),
                    },
                };
                if let Err(e) = msg.publish(self, swarm) {
                    tracing::warn!(
                        "{}: error requesting peers to provide {data}: {e}",
                        self.local_display_name
                    );
                }
            }
            for peer in current_peers {
                swarm.behaviour_mut().request_response.send_request(
                    &peer,
                    match data {
                        DataLabel::Block(height) => BlockRequest::BlockAtHeight(height),
                        DataLabel::Merkle(hash) => BlockRequest::Merkle(hash),
                    },
                );
            }
        }
    }

    // Returns true if the notfication should be skipped publishing to the p2p network
    fn notification_hash_exists(&self, hash: &Sha256Hash) -> bool {
        let expiry = { self.lru_notifications.read().peek(hash).cloned() };
        match expiry {
            Some(instant) => {
                let elapsed = instant.elapsed();
                if elapsed > Duration::from_secs(60) {
                    // Let's allow it to be submitted to the gossip network again
                    false
                } else {
                    true
                }
            }
            None => false,
        }
    }

    fn update_notification_lru(&self, hash: Sha256Hash) {
        let mut lru = self.lru_notifications.write();
        lru.push(hash, Instant::now());
    }

    fn check_and_update_notification_lru(&self, hash: &Sha256Hash) -> bool {
        let exists = self.notification_hash_exists(hash);
        if !exists {
            self.update_notification_lru(*hash);
        }
        exists
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

/// Helper data type that only displays the first 8 characters of a hash.
struct FirstEightChars(Sha256Hash);

impl Display for FirstEightChars {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", hex::encode(&self.0.as_array()[0..4]))
    }
}

#[cfg(test)]
mod tests {
    use merkle_map::Sha256Hash;

    use super::FirstEightChars;

    quickcheck::quickcheck! {
        fn first_eight_chars(input: Vec<u8>) -> bool {
            first_eight_chars_helper(input);
            true
        }
    }

    fn first_eight_chars_helper(input: Vec<u8>) {
        let hash = Sha256Hash::hash(&input);
        let expected = hash.to_string().chars().take(8).collect::<String>();
        let actual = FirstEightChars(hash).to_string();
        assert_eq!(expected, actual);
    }
}
