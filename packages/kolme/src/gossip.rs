mod messages;
mod state_sync;

use std::{
    fmt::Display,
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    time::Duration,
};

use crate::*;
use messages::*;

use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{
    futures::StreamExt,
    gossipsub::{self, IdentTopic},
    mdns, noise,
    request_response::{ProtocolSupport, ResponseChannel},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, StreamProtocol, Swarm, SwarmBuilder,
};
use state_sync::{DataRequest, StateSyncStatus};
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
    /// Trigger a check of the state sync.
    trigger_state_sync: Trigger,
    /// Switches to true once we have our first success received message
    watch_network_ready: tokio::sync::watch::Sender<bool>,
    /// Status of state syncs, if present.
    state_sync: Mutex<StateSyncStatus<App>>,
}

// We create a custom network behaviour that combines Gossipsub and Mdns.
#[derive(NetworkBehaviour)]
struct KolmeBehaviour<AppMessage: serde::de::DeserializeOwned + Send + Sync + 'static> {
    gossipsub: gossipsub::Behaviour,
    request_response:
        libp2p::request_response::cbor::Behaviour<BlockRequest, BlockResponse<AppMessage>>,
    mdns: Toggle<mdns::tokio::Behaviour>,
    kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
}

pub struct GossipBuilder {
    keypair: Option<Keypair>,
    bootstrap: Vec<(PeerId, Multiaddr)>,
    disable_quic: bool,
    disable_tcp: bool,
    disable_ip4: bool,
    disable_ip6: bool,
    disable_mdns: bool,
    listen_ports: Vec<u16>,
    /// See [libp2p::gossipsub::Configbuilder::heartbeat_interval]
    heartbeat_interval: Duration,
    sync_mode: SyncMode,
    data_load_validation: DataLoadValidation,
    local_display_name: Option<String>,
}

impl Default for GossipBuilder {
    fn default() -> Self {
        Self {
            keypair: Default::default(),
            bootstrap: Default::default(),
            disable_quic: Default::default(),
            disable_tcp: Default::default(),
            disable_ip4: Default::default(),
            disable_ip6: Default::default(),
            disable_mdns: Default::default(),
            listen_ports: Default::default(),
            // This is set to aid debugging by not cluttering the log space
            heartbeat_interval: Duration::from_secs(10),
            sync_mode: Default::default(),
            data_load_validation: Default::default(),
            local_display_name: Default::default(),
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
    #[default]
    StateTransfer,
    /// Allow state transfer for version upgrades, but otherwise use block sync.
    StateTransferForUpgrade,
    /// Always do block sync, verifying each new block
    BlockTransfer,
}

impl GossipBuilder {
    pub fn new() -> Self {
        Self {
            disable_ip6: true,
            disable_quic: true,
            ..Self::default()
        }
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

    pub fn disable_mdns(mut self) -> Self {
        self.disable_mdns = true;
        self
    }

    /// Add a listen port
    ///
    /// If none are provided, a random port is selected per interface
    pub fn add_listen_port(mut self, port: u16) -> Self {
        self.listen_ports.push(port);
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

    pub fn build<App: KolmeApp>(self, kolme: Kolme<App>) -> Result<Gossip<App>> {
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

                let mdns = if !self.disable_mdns {
                    Some(mdns::tokio::Behaviour::new(
                        mdns::Config::default(),
                        key.public().to_peer_id(),
                    )?)
                } else {
                    None
                };
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
                    mdns: Toggle::from(mdns),
                    request_response,
                    kademlia,
                })
            })?
            .build();

        // Create the Gossipsub topics
        let genesis_hash = FirstEightChars(kolme.get_genesis_hash()?);
        let gossip_topic = gossipsub::IdentTopic::new(format!("/kolme-gossip/{genesis_hash}/1.0"));
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

        let (watch_network_ready, _) = tokio::sync::watch::channel(false);
        let local_peer_id = *swarm.local_peer_id();
        let state_sync = Mutex::new(StateSyncStatus::new(kolme.clone()));

        Ok(Gossip {
            kolme,
            swarm: Mutex::new(swarm),
            gossip_topic,
            sync_mode: self.sync_mode,
            data_load_validation: self.data_load_validation,
            local_peer_id,
            trigger_broadcast_height: Trigger::new("broadcast_height"),
            trigger_state_sync: Trigger::new("state_sync"),
            watch_network_ready,
            local_display_name: self.local_display_name.unwrap_or(String::from("gossip")),
            state_sync,
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

        let (peers_with_blocks_tx, mut peers_with_blocks_rx) = tokio::sync::mpsc::channel(16);

        // Interval for broadcasting our block height
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.reset_immediately();
        let mut trigger_broadcast_height = self.trigger_broadcast_height.subscribe();
        let mut trigger_state_sync = self.trigger_state_sync.subscribe();

        let mut mempool_additions = self.kolme.subscribe_mempool_additions();

        let (block_requester, mut block_requester_rx) = tokio::sync::mpsc::channel(8);
        self.kolme.set_block_requester(block_requester);

        loop {
            tokio::select! {
                // Our local Kolme generated a notification to be sent through the
                // rest of the p2p network
                notification = subscription.recv() =>
                async {self.handle_notification(&mut swarm, notification)}.await,
                // A new event was generated from the p2p network
                event = swarm.select_next_some() => self.handle_event(&mut swarm, event, &peers_with_blocks_tx).await,
                // A peer reported a known height higher than we have, so
                // try to synchronize with it
                report_block_height = peers_with_blocks_rx.recv() => async {self.catch_up(&mut swarm,report_block_height).await}.await,
                // Periodically notify the p2p network of our latest block height
                // Also use this time to broadcast any transactions from the mempool.
                _ = interval.tick() => async {
                    self.request_block_heights(&mut swarm);
                    self.process_state_sync(&mut swarm).await;
                    self.broadcast_latest_block(&mut swarm);
                    self.broadcast_mempool_entries(&mut swarm);
                }.await,
                // When we're specifically triggered for it, also notify for latest block height
                _ = trigger_broadcast_height.listen() => async {self.broadcast_latest_block(&mut swarm)}.await,
                // Same with state sync
                _ = trigger_state_sync.listen() => async { self.process_state_sync(&mut swarm).await }.await,
                // And any time we add something new to the mempool, broadcast all items.
                _ = mempool_additions.listen() => self.broadcast_mempool_entries(&mut swarm),
                Some(height) = block_requester_rx.recv() => {
                    if let Err(e) = self.state_sync.lock().await.add_needed_block(height, None).await {
                        tracing::warn!("{}: error when adding requested block {height}: {e}", self.local_display_name)
                    } else {
                        self.trigger_state_sync.trigger();
                    }
                }
            }
        }
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
        for tx in self.kolme.get_mempool_entries() {
            let txhash = tx.hash();
            let msg = GossipMessage::BroadcastTx { tx };
            if let Err(e) = msg.publish(self, swarm) {
                tracing::error!(
                    "{}: Unable to broadcast transaction {txhash}: {e:?}",
                    self.local_display_name
                )
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
        peers_with_blocks: &tokio::sync::mpsc::Sender<ReportBlockHeight>,
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
            SwarmEvent::Behaviour(KolmeBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer, address) in peers {
                    tracing::info!(
                        "{local_display_name}: Discovered new peer over mDNS: {peer} @ {address}"
                    );
                    // TODO do we need to manually add mDNS peers to Kademlia?
                    swarm.behaviour_mut().kademlia.add_address(&peer, address);
                }
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
                        if let Err(e) = self.handle_message(message, peers_with_blocks, swarm).await
                        {
                            tracing::warn!(
                                "{local_display_name}: Error while handling message: {e}"
                            );
                        }
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
            _ => tracing::debug!(
                "{local_display_name}: Received and ignoring libp2p event: {event:?}"
            ),
        }
    }

    async fn handle_message(
        &self,
        message: GossipMessage<App>,
        peers_with_blocks: &tokio::sync::mpsc::Sender<ReportBlockHeight>,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
    ) -> Result<()> {
        let local_display_name = self.local_display_name.clone();
        match message {
            GossipMessage::Notification(msg) => {
                tracing::debug!("{local_display_name}: got notification message");
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
                    Notification::LatestBlock(_) => (),
                }
                self.kolme.notify(msg);
            }
            GossipMessage::RequestBlockHeights(_) => {
                tracing::debug!("{local_display_name}: got block heights request message");
                self.trigger_broadcast_height.trigger();
            }
            GossipMessage::ReportBlockHeight(report) => {
                tracing::debug!("{local_display_name}: got block height report message");
                let our_next = self.kolme.read().get_next_height();
                tracing::debug!(
                    "{local_display_name}: Received ReportBlockHeight: {report:?}, our_next: {our_next}"
                );
                // Check if this peer has new blocks that we'd want to request.
                if our_next < report.next {
                    peers_with_blocks.try_send(report).ok();
                }
            }
            GossipMessage::BroadcastTx { tx } => {
                self.kolme.propose_transaction(tx);
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

        Ok(())
    }

    async fn handle_request(
        &self,
        request: BlockRequest,
        channel: ResponseChannel<BlockResponse<App::Message>>,
        swarm: &mut Swarm<KolmeBehaviour<App::Message>>,
    ) {
        let local_display_name = self.local_display_name.clone();
        match request {
            BlockRequest::BlockAtHeight(height) => {
                let res = match self.kolme.read().get_block(height).await {
                    Err(e) => {
                        tracing::warn!("{local_display_name}: Error querying block in gossip: {e}");
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
                    tracing::warn!(
                        "{local_display_name}: Unable to answer BlockAtHeight request: {e:?}"
                    );
                }
            }
            BlockRequest::BlockWithStateAtHeight(height) => {
                let res = match self.kolme.read().get_block(height).await {
                    Err(e) => {
                        tracing::warn!(
                            "{local_display_name}: Error querying block (with state) in gossip: {e}"
                        );
                        return;
                    }
                    Ok(None) => BlockResponse::HeightNotFound(height),
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
                        BlockResponse::BlockWithState {
                            block: storable_block.block,
                        }
                    }
                };
                if let Err(e) = swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, res)
                {
                    tracing::warn!(
                        "{local_display_name}: Unable to answer BlockWithStateAtHeight request: {e:?}"
                    );
                }
            }
            BlockRequest::Merkle(hash) => match self.kolme.get_merkle_layer(hash).await {
                // We didn't have it, in theory we could send a message back about this, but
                // skipping for now
                Ok(None) => {
                    tracing::warn!("{local_display_name}: Received a request for merkle layer {hash}, but didn't have it.");
                }
                Ok(Some(contents)) => {
                    if let Err(e) = swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, BlockResponse::Merkle { hash, contents })
                    {
                        tracing::warn!(
                            "{local_display_name}: Unable to answer Merkle request: {e:?}"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "{local_display_name}: Error when loading Merkle layer for {hash}: {e}"
                    )
                }
            },
            BlockRequest::BlockAvailable { height, peer } => {
                self.state_sync.lock().await.add_block_peer(height, peer);
                self.trigger_state_sync.trigger();
            }
            BlockRequest::LayerAvailable { hash, peer } => {
                self.state_sync.lock().await.add_layer_peer(hash, peer);
                self.trigger_state_sync.trigger();
            }
        }
    }

    async fn handle_response(&self, response: BlockResponse<App::Message>, peer: PeerId) {
        let local_display_name = self.local_display_name.clone();
        tracing::debug!("{local_display_name}: response");
        match response {
            BlockResponse::Block(block) => {
                self.add_block(block).await;
            }
            BlockResponse::BlockWithState { block } => {
                match self
                    .state_sync
                    .lock()
                    .await
                    .add_pending_block(block, peer)
                    .await
                {
                    Ok(()) => self.trigger_state_sync.trigger(),
                    Err(e) => {
                        tracing::error!("{local_display_name}: error adding pending block: {e}")
                    }
                }
            }
            BlockResponse::HeightNotFound(height) => {
                tracing::warn!(
                    "{local_display_name}: Tried to find block height {height}, but peer didn't find it"
                );
            }
            BlockResponse::Merkle { hash, contents } => {
                match self
                    .state_sync
                    .lock()
                    .await
                    .add_merkle_layer(hash, contents, peer)
                    .await
                {
                    Ok(()) => self.trigger_state_sync.trigger(),
                    Err(e) => tracing::error!(
                        "{local_display_name}: error adding Merkle layer contents for {hash}: {e}"
                    ),
                }
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
            latest_block,
        } = match report_block_height {
            Some(report) => report,
            None => return,
        };

        let our_next = self.kolme.read().get_next_height();

        tracing::debug!(
            "{}: In catch_up, their_node=={their_next}, peer=={peer}, our_next=={our_next}",
            self.local_display_name
        );

        let their_highest = match their_next.prev() {
            None => return,
            Some(highest) => highest,
        };

        if their_highest < our_next {
            // They don't have any new blocks for us.
            return;
        }

        if let Some(latest_block) = latest_block {
            self.kolme.notify(Notification::LatestBlock(latest_block));
        }

        let do_state = match self.sync_mode {
            // Only do a state transfer if we've fallen more than 1 block behind.
            SyncMode::StateTransfer => their_highest != our_next,
            SyncMode::StateTransferForUpgrade => {
                todo!("Holding off on StateTransferForUpgrade until we handle upgrades")
            }
            SyncMode::BlockTransfer => {
                // For now, we force a state transfer if the chain and
                // code versions mismatch. In the future, we may decide
                // to be a bit more selective about this for security.
                self.kolme.get_code_version() != self.kolme.read().get_chain_version()
            }
        };

        if do_state {
            match self
                .state_sync
                .lock()
                .await
                .add_needed_block(their_highest, Some(peer))
                .await
            {
                Ok(()) => self.trigger_state_sync.trigger(),
                Err(e) => tracing::error!(
                    "{}: error adding needed block: {e}",
                    self.local_display_name
                ),
            }
        } else {
            swarm
                .behaviour_mut()
                .request_response
                .send_request(&peer, BlockRequest::BlockAtHeight(our_next));
        }
    }

    async fn add_block(&self, block: Arc<SignedBlock<App::Message>>) {
        // Don't add blocks from different versions
        if self.kolme.get_code_version() != self.kolme.read().get_chain_version() {
            return;
        }
        if block.0.message.as_inner().height == self.kolme.read().get_next_height() {
            if let Err(e) = self
                .kolme
                .add_block_with(block, self.data_load_validation)
                .await
            {
                tracing::warn!(
                    "{}: Unable to add block to chain: {e}",
                    self.local_display_name
                )
            }
        }
    }

    async fn process_state_sync(&self, swarm: &mut Swarm<KolmeBehaviour<App::Message>>) {
        let requests = match self.state_sync.lock().await.get_requests().await {
            Ok(requests) => requests,
            Err(e) => {
                tracing::error!(
                    "{}: unable to get state sync requests: {e}",
                    self.local_display_name
                );
                return;
            }
        };
        for request in requests {
            match request {
                DataRequest::GetBlock {
                    height,
                    current_peers,
                    request_new_peers,
                } => {
                    if request_new_peers || current_peers.is_empty() {
                        let msg = GossipMessage::RequestBlockContents {
                            height,
                            peer: self.peer_id(),
                        };
                        if let Err(e) = msg.publish(self, swarm) {
                            tracing::warn!(
                                "{}: error requesting block contents for {height}: {e}",
                                self.local_display_name
                            );
                        }
                    }
                    for peer in current_peers {
                        swarm
                            .behaviour_mut()
                            .request_response
                            .send_request(&peer, BlockRequest::BlockWithStateAtHeight(height));
                    }
                }
                DataRequest::GetMerkle {
                    hash,
                    current_peers,
                    request_new_peers,
                } => {
                    if request_new_peers || current_peers.is_empty() {
                        let msg = GossipMessage::RequestLayerContents {
                            hash,
                            peer: self.peer_id(),
                        };
                        if let Err(e) = msg.publish(self, swarm) {
                            tracing::warn!(
                                "{}: error requesting layer contents for {hash}: {e}",
                                self.local_display_name
                            );
                        }
                    }
                    for peer in current_peers {
                        swarm
                            .behaviour_mut()
                            .request_response
                            .send_request(&peer, BlockRequest::Merkle(hash));
                    }
                }
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
