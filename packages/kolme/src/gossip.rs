mod messages;
mod sync_manager;
mod websockets;

use std::{net::SocketAddr, time::Duration};

use crate::*;
use messages::*;

use sync_manager::{DataRequest, SyncManager};
use tokio::{sync::Mutex, task::JoinSet};

use utils::trigger::Trigger;
use websockets::{WebsocketsManager, WebsocketsMessage};

/// A component that retrieves notifications from the network and broadcasts our own notifications back out.
pub struct Gossip<App: KolmeApp> {
    kolme: Kolme<App>,
    sync_mode: SyncMode,
    data_load_validation: DataLoadValidation,
    // human-readable name for an instance
    local_display_name: String,
    /// The block sync manager.
    sync_manager: Mutex<SyncManager<App>>,
    concurrent_request_limit: usize,
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
            local_display_name,
            sync_manager,
            concurrent_request_limit: self.concurrent_request_limit,
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
        // Interval for broadcasting our block height
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.reset_immediately();

        let mut mempool_additions = self.kolme.subscribe_mempool_additions();

        let (block_requester, mut block_requester_rx) = tokio::sync::mpsc::channel(8);
        self.kolme.set_block_requester(block_requester);

        let mut sync_manager_subscriber = self.sync_manager.lock().await.subscribe();

        let mut latest_watch = self.kolme.subscribe_latest_block();

        loop {
            tokio::select! {
                // Periodically notify the p2p network of our latest block height
                // Also use this time to broadcast any transactions from the mempool.
                _ = interval.tick() => {
                    self.on_interval().await;
                },
                _ = sync_manager_subscriber.listen() => {
                    self.process_sync_manager().await;
                }
                // And any time we add something new to the mempool, broadcast all items.
                _ = mempool_additions.listen() => {
                    self.broadcast_mempool_entries();
                }
                msg = self.websockets_manager.get_incoming() => {
                    self.handle_message(msg).await;
                }
                Some(height) = block_requester_rx.recv() => {
                    if let Err(e) = self.sync_manager.lock().await.add_needed_block(&self, height).await {
                        tracing::warn!("{}: error when adding requested block {height}: {e}", self.local_display_name)
                    }
                }
                latest = latest_watch.changed() => {
                    #[cfg(debug_assertions)]
                    latest.unwrap();
                    self.update_latest().await;
                }
            }
        }
    }

    async fn on_interval(&self) {
        self.process_sync_manager().await;
        self.broadcast_mempool_entries();
    }

    fn broadcast_mempool_entries(&self) {
        for tx in self.kolme.get_mempool_entries_for_gossip() {
            let txhash = tx.hash();
            let msg = GossipMessage::BroadcastTx { tx };
            msg.publish(self);
            self.kolme.mark_mempool_entry_gossiped(txhash);
        }
    }

    async fn update_latest(&self) {
        let Some(latest) = self.kolme.get_latest_block() else {
            return;
        };
        self.sync_manager
            .lock()
            .await
            .add_latest_block(&self, latest.message.as_inner())
            .await;
        GossipMessage::ProvideLatestBlock { latest }.publish(self);
    }

    async fn handle_message(
        &self,
        WebsocketsMessage {
            payload: message,
            tx: ws_sender,
        }: WebsocketsMessage<App>,
    ) {
        let local_display_name = self.local_display_name.clone();
        match message {
            GossipMessage::ProvideLatestBlock { latest } => {
                self.kolme.update_latest_block(latest);
            }
            GossipMessage::RequestBlock { height } => {
                let block = match self.kolme.get_block(height).await {
                    Err(e) => {
                        tracing::warn!(
                            "{local_display_name}: RequestBlockContents error on {height}: {e}"
                        );
                        return;
                    }
                    Ok(block) => block,
                };
                let msg = GossipMessage::ProvideBlock {
                    height,
                    block: block.map(|s| s.block),
                };
                if let Err(e) = ws_sender.tx.send(msg).await {
                    tracing::error!("Unexpected error on ws_sender.send ProvideBlock: {e}");
                }
            }
            GossipMessage::ProvideBlock { height: _, block } => {
                // FIXME verify signature?
                if let Some(block) = block {
                    self.sync_manager
                        .lock()
                        .await
                        .add_pending_block(self, block)
                        .await;
                }
            }
            GossipMessage::RequestLayer { hash } => {
                let contents = match self.kolme.get_merkle_layer(hash).await {
                    Err(e) => {
                        tracing::warn!(
                            "{local_display_name}: RequestLayerContents error on {hash}: {e}"
                        );
                        return;
                    }
                    Ok(contents) => contents,
                };
                let msg = GossipMessage::ProvideLayer {
                    hash,
                    contents: contents.map(Arc::new),
                };
                if let Err(e) = ws_sender.tx.send(msg).await {
                    tracing::error!("Unexpected error on ws_sender.send ProvideLayer: {e}");
                }
            }
            GossipMessage::ProvideLayer { hash, contents } => {
                // FIXME verify hash?
                if let Some(contents) = contents {
                    if let Err(e) = self
                        .sync_manager
                        .lock()
                        .await
                        .add_merkle_layer(self, hash, contents)
                        .await
                    {
                        tracing::error!(%local_display_name, "Unable to add Merkle layer {hash}: {e}");
                    }
                }
            }
            GossipMessage::BroadcastTx { tx } => {
                let txhash = tx.hash();
                match self.kolme.propose_transaction(tx) {
                    Ok(()) => {}
                    Err(_) => todo!(),
                }
                self.kolme.mark_mempool_entry_gossiped(txhash);
            }
            GossipMessage::FailedTransaction { failed } => {
                self.kolme.add_failed_transaction(failed);
            }
        }
        // match message {
        //     GossipMessage::Notification(msg) => {
        //         match &msg {
        //             Notification::EvictMempoolTransaction(signed_json) => {
        //                 let pubkey = signed_json.verify_signature();
        //                 match pubkey {
        //                     Ok(pubkey) => {
        //                         let processor = self
        //                             .kolme
        //                             .read()
        //                             .get_framework_state()
        //                             .get_validator_set()
        //                             .processor;
        //                         if pubkey != processor {
        //                             tracing::warn!("Evict transaction was signed by {pubkey}, but processor is {processor}");
        //                         } else {
        //                             let txhash = signed_json.message.as_inner();
        //                             tracing::debug!("Transaction {txhash} evicted from mempool");
        //                             self.kolme.remove_from_mempool(*txhash);
        //                         }
        //                     }
        //                     Err(e) => {
        //                         tracing::warn!("Error verifying evict mempool signature: {e}")
        //                     }
        //                 }
        //             }
        //         }
        //         self.kolme.notify(msg);
        //     }
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
        for request in requests {
            let msg = match request {
                DataRequest::Block(height) => GossipMessage::RequestBlock { height },
                DataRequest::Merkle(hash) => GossipMessage::RequestLayer { hash },
            };
            msg.publish(self);
        }
    }
}
