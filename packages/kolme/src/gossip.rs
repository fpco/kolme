mod messages;
mod sync_manager;
mod websockets;

use std::{
    net::SocketAddr,
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use crate::*;
use messages::*;

use sync_manager::{DataLabel, DataRequest, SyncManager};
use tokio::{
    sync::{broadcast::error::RecvError, Mutex},
    task::JoinSet,
};

use utils::trigger::Trigger;
use websockets::{WebsocketsManager, WebsocketsMessage, WebsocketsPrivateSender};

/// A component that retrieves notifications from the network and broadcasts our own notifications back out.
pub struct Gossip<App: KolmeApp> {
    kolme: Kolme<App>,
    sync_mode: SyncMode,
    data_load_validation: DataLoadValidation,
    // human-readable name for an instance
    local_display_name: String,
    /// Trigger a broadcast of our latest block height.
    trigger_broadcast_height: Trigger,
    /// The block sync manager.
    sync_manager: Mutex<SyncManager<App>>,
    /// LRU cache for notifications received via P2p layer
    lru_notifications: parking_lot::RwLock<lru::LruCache<Sha256Hash, Instant>>,
    concurrent_request_limit: usize,
    max_peer_count: usize,
    warning_period: Duration,
    websockets_manager: WebsocketsManager<App>,
    set: Option<JoinSet<()>>,
}

pub struct GossipBuilder {
    sync_mode: SyncMode,
    data_load_validation: DataLoadValidation,
    local_display_name: Option<String>,
    duplicate_cache_time: Duration,
    concurrent_request_limit: usize,
    max_peer_count: usize,
    warning_period: Duration,
    websockets_binds: Vec<SocketAddr>,
    websockets_servers: Vec<String>,
}

impl Default for GossipBuilder {
    fn default() -> Self {
        Self {
            sync_mode: Default::default(),
            data_load_validation: Default::default(),
            local_display_name: Default::default(),
            // Same default as libp2p_gossip
            duplicate_cache_time: Duration::from_secs(60),
            concurrent_request_limit: sync_manager::DEFAULT_REQUEST_COUNT,
            max_peer_count: sync_manager::DEFAULT_PEER_COUNT,
            warning_period: Duration::from_secs(sync_manager::DEFAULT_WARNING_PERIOD_SECS),
            websockets_binds: vec![],
            websockets_servers: vec![],
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

    pub fn set_duplicate_cache_time(mut self, cache_time: Duration) -> Self {
        self.duplicate_cache_time = cache_time;
        self
    }

    pub fn add_websockets_bind(mut self, bind: SocketAddr) -> Self {
        self.websockets_binds.push(bind);
        self
    }

    pub fn add_websockets_server(mut self, host: impl Into<String>) -> Self {
        self.websockets_servers.push(host.into());
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
        // Create the Gossipsub topics
        tracing::info!(
            "Genesis info: {}",
            serde_json::to_string(kolme.get_genesis_info())?
        );

        let local_display_name = self.local_display_name.unwrap_or(String::from("gossip"));

        let sync_manager = Mutex::new(SyncManager::default());
        let mut set = JoinSet::new();
        let websockets_manager = WebsocketsManager::new(
            &mut set,
            self.websockets_binds,
            self.websockets_servers,
            &local_display_name,
        )?;

        Ok(Gossip {
            kolme,
            sync_mode: self.sync_mode,
            data_load_validation: self.data_load_validation,
            trigger_broadcast_height: Trigger::new("broadcast_height"),
            local_display_name,
            sync_manager,
            lru_notifications: lru::LruCache::new(NonZeroUsize::new(100).unwrap()).into(),
            concurrent_request_limit: self.concurrent_request_limit,
            max_peer_count: self.max_peer_count,
            warning_period: self.warning_period,
            websockets_manager,
            set: Some(set),
        })
    }
}

impl<App: KolmeApp> Gossip<App> {
    pub async fn run(mut self) -> Result<()> {
        let mut set = self.set.take().unwrap();
        set.spawn(self.run_inner());
        let res = set.join_next().await;
        panic!("Unexpected exit in gossip: {res:?}");
    }

    async fn run_inner(mut self) {
        let mut subscription = self.kolme.subscribe();

        // Interval for broadcasting our block height
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.reset_immediately();
        let mut trigger_broadcast_height = self.trigger_broadcast_height.subscribe();

        let mut mempool_additions = self.kolme.subscribe_mempool_additions();

        let (block_requester, mut block_requester_rx) = tokio::sync::mpsc::channel(8);
        self.kolme.set_block_requester(block_requester);

        let mut sync_manager_subscriber = self.sync_manager.lock().await.subscribe();

        loop {
            tokio::select! {
                // Our local Kolme generated a notification to be sent through the
                // rest of the p2p network
                notification = subscription.recv() => {
                    self.handle_notification(notification);
                }
                // Periodically notify the p2p network of our latest block height
                // Also use this time to broadcast any transactions from the mempool.
                _ = interval.tick() => {
                    self.on_interval().await;
                },
                _ = sync_manager_subscriber.listen() => {
                    self.process_sync_manager().await;
                }
                // When we're specifically triggered for it, also notify for latest block height
                _ = trigger_broadcast_height.listen() => {
                    self.broadcast_latest_block();
                }
                // And any time we add something new to the mempool, broadcast all items.
                _ = mempool_additions.listen() => {
                    self.broadcast_mempool_entries();
                }
                msg = self.websockets_manager.get_incoming() => {
                    self.handle_websockets_message(msg).await;
                }
                Some(height) = block_requester_rx.recv() => {
                    if let Err(e) = self.sync_manager.lock().await.add_needed_block(&self, height, None).await {
                        tracing::warn!("{}: error when adding requested block {height}: {e}", self.local_display_name)
                    }
                }
            }
        }
    }

    async fn on_interval(&self) {
        self.process_sync_manager().await;
        self.broadcast_latest_block();
        self.broadcast_mempool_entries();
    }

    fn broadcast_latest_block(&self) {
        GossipMessage::ReportBlockHeight(ReportBlockHeight {
            next: self.kolme.read().get_next_height(),
            timestamp: jiff::Timestamp::now(),
            latest_block: self.kolme.get_latest_block(),
        })
        .publish(self);
    }

    fn broadcast_mempool_entries(&self) {
        for tx in self.kolme.get_mempool_entries_for_gossip() {
            let txhash = tx.hash();
            let msg = GossipMessage::BroadcastTx { tx };
            msg.publish(self);
            self.kolme.mark_mempool_entry_gossiped(txhash);
        }
    }

    fn handle_notification(&self, notification: Result<Notification<App::Message>, RecvError>) {
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
        GossipMessage::Notification(notification).publish(self);
    }

    async fn handle_message(
        &self,
        message: GossipMessage<App>,
        ws_sender: WebsocketsPrivateSender<App>,
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
                    .add_report_block_height(self, report, ws_sender)
                    .await;
            }
            GossipMessage::BroadcastTx { tx } => {
                let txhash = tx.hash();
                self.kolme.propose_transaction(tx);
                self.kolme.mark_mempool_entry_gossiped(txhash);
            }
            GossipMessage::RequestBlockContents { height } => {
                match self.kolme.has_block(height).await {
                    Err(e) => {
                        tracing::warn!(
                            "{local_display_name}: RequestBlockContents error on {height}: {e}"
                        );
                    }
                    Ok(false) => (),
                    Ok(true) => {
                        let req = BlockRequest::BlockAvailable { height };
                        if let Err(e) = ws_sender
                            .tx
                            .send(websockets::WebsocketsPayload::Request(req))
                            .await
                        {
                            tracing::error!(
                                "Unexpected error on ws_sender.send BlockAvailable: {e}"
                            );
                        }
                    }
                }
            }
            GossipMessage::RequestLayerContents { hash } => {
                match self.kolme.has_merkle_hash(hash).await {
                    Err(e) => tracing::warn!(
                        "{local_display_name}: RequestLayerContents error on {hash}: {e}"
                    ),
                    Ok(false) => (),
                    Ok(true) => {
                        let req = BlockRequest::LayerAvailable { hash };
                        if let Err(e) = ws_sender
                            .tx
                            .send(websockets::WebsocketsPayload::Request(req))
                            .await
                        {
                            tracing::error!(
                                "Unexpected error on ws_sender.send LayerAvailable: {e}"
                            );
                        }
                    }
                }
            }
        }
    }

    async fn handle_websockets_message(&self, msg: WebsocketsMessage<App>) {
        match msg.payload {
            websockets::WebsocketsPayload::Gossip(message) => {
                self.handle_message(message, msg.tx).await
            }
            websockets::WebsocketsPayload::Request(request) => {
                let res = self.handle_request_helper(request, msg.tx.clone()).await;
                if let Err(e) = msg
                    .tx
                    .tx
                    .send(websockets::WebsocketsPayload::Response(res))
                    .await
                {
                    tracing::error!(
                        "handle_websockets_message: unexpected error sending response: {e}"
                    );
                }
            }
            websockets::WebsocketsPayload::Response(response) => {
                self.handle_response(response, msg.tx).await;
            }
        }
    }

    async fn handle_request_helper(
        &self,
        request: BlockRequest,
        ws_sender: WebsocketsPrivateSender<App>,
    ) -> BlockResponse<App::Message> {
        let local_display_name = self.local_display_name.clone();
        match &request {
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
            BlockRequest::BlockAvailable { height } => {
                self.sync_manager
                    .lock()
                    .await
                    .add_block_peer(self, *height, ws_sender);
                BlockResponse::Ack
            }
            BlockRequest::LayerAvailable { hash } => {
                self.sync_manager
                    .lock()
                    .await
                    .add_layer_peer(self, *hash, ws_sender);
                BlockResponse::Ack
            }
        }
    }

    async fn handle_response(
        &self,
        response: BlockResponse<App::Message>,
        peer: WebsocketsPrivateSender<App>,
    ) {
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
                    "{local_display_name}: Tried to find block height {height}, but peer {peer:?} didn't find it"
                );
                self.sync_manager
                    .lock()
                    .await
                    .remove_block_peer(height, peer);
            }
            BlockResponse::MerkleNotFound(hash) => {
                tracing::warn!(
                    "{local_display_name}: Tried to find merkle layer {hash}, but peer {peer:?} didn't find it"
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

    async fn process_sync_manager(&self) {
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
                    DataLabel::Block(height) => GossipMessage::RequestBlockContents { height },
                    DataLabel::Merkle(hash) => GossipMessage::RequestLayerContents { hash },
                };
                msg.publish(self);
            }
            for ws in current_peers {
                let req = match data {
                    DataLabel::Block(height) => BlockRequest::BlockAtHeight(height),
                    DataLabel::Merkle(hash) => BlockRequest::Merkle(hash),
                };
                if let Err(e) = ws
                    .tx
                    .send(websockets::WebsocketsPayload::Request(req))
                    .await
                {
                    tracing::warn!("{}: unable to send {req:?}: {e}", self.local_display_name);
                }
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
