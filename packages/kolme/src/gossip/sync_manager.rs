// ! Implementation of the state sync state machine.
//
// Due to the complexity of the topic, there's a dedicated doc for this on the docs
// site called "State sync implementation". Recommendation: read that before reading
// the code below.
use std::{
    collections::{hash_map, HashMap, HashSet},
    fmt::Display,
    time::Instant,
};

use gossip::{ReportBlockHeight, Trigger};
use libp2p::PeerId;
use smallvec::SmallVec;
use utils::trigger::TriggerSubscriber;

use crate::*;

/// Status of state syncing.
pub(super) struct SyncManager<App: KolmeApp> {
    /// Blocks we're currently waiting on.
    needed_blocks: BTreeMap<BlockHeight, WaitingBlock<App::Message>>,
    /// Blocks that need a given layer.
    reverse_blocks: HashMap<Sha256Hash, HashSet<BlockHeight>>,
    /// Trigger [Gossip] to recheck sync requests.
    trigger: Trigger,
}

#[allow(clippy::large_enum_variant)]
enum WaitingBlock<AppMessage> {
    /// We haven't received the basic block info yet.
    Needed(RequestStatus),
    /// We have the raw block, but haven't processed it yet.
    Received {
        block: Arc<SignedBlock<AppMessage>>,
        peer: Option<PeerId>,
    },
    /// We've received the block info, but now need to download the state layers.
    Pending(PendingBlock<AppMessage>),
}

#[derive(Debug)]
struct PendingBlock<AppMessage> {
    block: Arc<SignedBlock<AppMessage>>,
    /// Layers we're interested in but haven't received any info on yet.
    needed_layers: BTreeMap<Sha256Hash, RequestStatus>,
    /// Merkle layers that don't yet have the full content for the children.
    pending_layers: HashMap<Sha256Hash, MerkleLayerContents>,
    /// A reverse map for pending layers: parents waiting on children.
    ///
    /// Every time we discover a layer with unknown children, we fill in
    /// both pending_layers and this field. When we complete a layer,
    /// we can look up the parents in this map and see if they were completed
    /// too.
    reverse_layers: HashMap<Sha256Hash, HashSet<Sha256Hash>>,
}

#[derive(Debug)]
pub(super) struct DataRequest {
    pub(super) data: DataLabel,
    pub(super) current_peers: SmallVec<[PeerId; REQUEST_COUNT]>,
    pub(super) request_new_peers: bool,
}

#[derive(Debug)]
enum ShouldRequest {
    DontRequest,
    RequestNoPeers,
    RequestWithPeers,
}

/// Status of a request for data.
#[derive(Debug)]
struct RequestStatus {
    /// When was the last time we made a request?
    ///
    /// This starts off as [None]. The first time we make a request, we use the original peer only. Thereafter, we always request new peers as well.
    last_sent: Option<Instant>,
    /// Peers we can query for the data.
    peers: SmallVec<[PeerId; REQUEST_COUNT]>,
    /// Last time we updated the warning state.
    ///
    /// Warning state is to track how long a request has been unanswered. We update it (1) the first time we make a request and (2) every time we print out a warning.
    warning_updated: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DataLabel {
    Block(BlockHeight),
    Merkle(Sha256Hash),
}

impl Display for DataLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DataLabel::Block(height) => write!(f, "block {height}"),
            DataLabel::Merkle(hash) => write!(f, "merkle layer {hash}"),
        }
    }
}

impl RequestStatus {
    fn new(peer: Option<PeerId>) -> Self {
        RequestStatus {
            last_sent: None,
            peers: peer.into_iter().collect(),
            warning_updated: None,
        }
    }

    fn add_peer(&mut self, peer: PeerId) {
        // Don't add a duplicate
        if self.peers.contains(&peer) {
            return;
        }

        // If we had no peers previously, reset the last_sent so
        // that we'll immediately make a request.
        if self.peers.is_empty() {
            self.last_sent = None;
        }

        if self.peers.len() >= REQUEST_COUNT {
            debug_assert!(self.peers.len() == REQUEST_COUNT);
            self.peers.remove(0);
        }
        self.peers.push(peer);
    }

    /// Check if we should try this request again.
    ///
    /// This will automatically update the [last_sent] field if a request
    /// should be made. It's also responsible for printing warnings for
    /// requests that are taking too long.
    fn should_request<App: KolmeApp>(
        &mut self,
        gossip: &Gossip<App>,
        label: DataLabel,
    ) -> ShouldRequest {
        let res = match self.last_sent {
            None => ShouldRequest::RequestNoPeers,
            Some(last_sent) => {
                if last_sent.elapsed().as_secs() < 2 {
                    ShouldRequest::DontRequest
                } else {
                    ShouldRequest::RequestWithPeers
                }
            }
        };

        let now = Instant::now();
        match self.warning_updated {
            None => self.warning_updated = Some(now),
            Some(warning_updated) => {
                if now.duration_since(warning_updated).as_secs() >= WARNING_PERIOD_SECS {
                    tracing::warn!(
                        "{}: still waiting on request for {label}",
                        gossip.local_display_name
                    );
                    self.warning_updated = Some(now);
                }
            }
        }
        match res {
            ShouldRequest::DontRequest => (),
            ShouldRequest::RequestNoPeers | ShouldRequest::RequestWithPeers => {
                self.last_sent = Some(Instant::now())
            }
        }
        res
    }

    fn remove_peer(&mut self, peer: PeerId) {
        self.peers.retain(|x| x != &peer);
    }
}

impl<App: KolmeApp> Default for SyncManager<App> {
    fn default() -> Self {
        SyncManager {
            needed_blocks: BTreeMap::new(),
            reverse_blocks: HashMap::new(),
            trigger: Trigger::new("gossip::sync_manager"),
        }
    }
}

impl<App: KolmeApp> SyncManager<App> {
    pub(super) fn subscribe(&self) -> TriggerSubscriber {
        self.trigger.subscribe()
    }

    pub(super) async fn add_needed_block(
        &mut self,
        gossip: &Gossip<App>,
        height: BlockHeight,
        peer: Option<PeerId>,
    ) -> Result<()> {
        if gossip.kolme.has_block(height).await? || self.needed_blocks.contains_key(&height) {
            return Ok(());
        }

        self.needed_blocks
            .insert(height, WaitingBlock::Needed(RequestStatus::new(peer)));
        self.trigger.trigger();
        Ok(())
    }

    pub(super) fn add_block_peer(&mut self, height: BlockHeight, peer: PeerId) {
        if let Some(WaitingBlock::Needed(status)) = self.needed_blocks.get_mut(&height) {
            status.add_peer(peer);
        }
    }

    pub(super) fn remove_block_peer(&mut self, height: BlockHeight, peer: PeerId) {
        if let Some(WaitingBlock::Needed(status)) = self.needed_blocks.get_mut(&height) {
            status.remove_peer(peer);
        }
    }

    pub(super) async fn add_pending_block(
        &mut self,
        gossip: &Gossip<App>,
        block: Arc<SignedBlock<App::Message>>,
        peer: Option<PeerId>,
    ) {
        let height = block.height();

        match gossip.kolme.has_block(height).await {
            Err(e) => {
                tracing::warn!(
                    "{}: add_pending_block: has_block({height}) failed: {e}",
                    gossip.local_display_name
                );
                return;
            }
            Ok(true) => {
                self.needed_blocks.remove(&height);
                return;
            }
            Ok(false) => (),
        }

        let Some(waiting) = self.needed_blocks.get_mut(&height) else {
            return;
        };

        match waiting {
            // We needed this block, so let's do it.
            WaitingBlock::Needed(_) => {
                *waiting = WaitingBlock::Received { block, peer };
                self.trigger.trigger();
            }
            // We already got it, nothing to do.
            WaitingBlock::Received { .. } | WaitingBlock::Pending(_) => (),
        }
    }

    //     let do_block = match gossip.sync_mode {
    //         // If we have the prior block and we have the right code version, still
    //         // do a block sync.
    //         // FIXME reconsider the StateTransferForUpgrade entirely, maybe it's too wonky. This is not the right implementation regardless.
    //         SyncMode::StateTransfer | SyncMode::StateTransferForUpgrade => {
    //             match height.prev() {
    //                 // We're at the starting block, so just confirm that we have
    //                 // the same code version as the genesis
    //                 None => {
    //                     &gossip.kolme.get_genesis_info().version == gossip.kolme.get_code_version()
    //                 }
    //                 Some(needed) => {
    //                     match gossip.kolme.load_block(needed).await {
    //                         Ok(block) => {
    //                             block.framework_state.get_chain_version()
    //                                 == gossip.kolme.get_code_version()
    //                         }
    //                         // For any errors, just go back to block sync
    //                         Err(_) => false,
    //                     }
    //                 }
    //             }
    //         }
    //         SyncMode::BlockTransfer => {
    //             // Block transfer mode: we never allow a state transfer
    //             true
    //         }
    //     };

    //     if do_block {
    //         // FIXME handle all the logic around requesting older blocks if needed
    //         gossip.kolme.add_block(block).await?;
    //     } else {
    //         let mut pending = PendingBlock {
    //             block,
    //             needed_layers: BTreeMap::new(),
    //             pending_layers: HashMap::new(),
    //             reverse_layers: HashMap::new(),
    //         };

    //         let inner = pending.block.0.message.as_inner();
    //         for hash in [inner.framework_state, inner.app_state, inner.logs] {
    //             if !gossip.kolme.has_merkle_hash(hash).await? {
    //                 pending.needed_layers.insert(hash, RequestStatus::new(peer));
    //                 self.reverse_blocks.entry(hash).or_default().insert(height);
    //             }
    //         }

    //         let has_all = pending.needed_layers.is_empty();
    //         *waiting = WaitingBlock::Pending(pending);
    //         if has_all {
    //             self.process_available_block(gossip, height).await?;
    //         }
    //     }

    //     Ok(())
    // }

    fn get_heights_for_layer(&self, hash: Sha256Hash) -> SmallVec<[BlockHeight; 4]> {
        self.reverse_blocks
            .get(&hash)
            .map_or_else(SmallVec::new, |set| set.iter().copied().collect())
    }

    pub(super) async fn add_merkle_layer(
        &mut self,
        gossip: &Gossip<App>,
        hash: Sha256Hash,
        layer: MerkleLayerContents,
        peer: PeerId,
    ) -> Result<()> {
        for height in self.get_heights_for_layer(hash) {
            self.add_merkle_layer_for(gossip, height, hash, &layer, peer)
                .await?;
        }

        self.reverse_blocks.remove(&hash);
        Ok(())
    }

    async fn add_merkle_layer_for(
        &mut self,
        gossip: &Gossip<App>,
        height: BlockHeight,
        hash: Sha256Hash,
        layer: &MerkleLayerContents,
        peer: PeerId,
    ) -> Result<()> {
        let Some(waiting) = self.needed_blocks.get_mut(&height) else {
            return Ok(());
        };

        let pending = match waiting {
            WaitingBlock::Needed(_) | WaitingBlock::Received { .. } => return Ok(()),
            WaitingBlock::Pending(pending) => pending,
        };

        if !pending.needed_layers.contains_key(&hash) || pending.pending_layers.contains_key(&hash)
        {
            return Ok(());
        }

        if !gossip.kolme.has_merkle_hash(hash).await? {
            let mut has_all = true;
            for child in &layer.children {
                if !gossip.kolme.has_merkle_hash(*child).await? {
                    has_all = false;
                    pending
                        .reverse_layers
                        .entry(*child)
                        .or_default()
                        .insert(hash);
                    self.reverse_blocks
                        .entry(*child)
                        .or_default()
                        .insert(height);
                    pending
                        .needed_layers
                        .entry(*child)
                        .or_insert(RequestStatus::new(Some(peer)));
                }
            }

            pending.pending_layers.insert(hash, layer.clone());
            pending.needed_layers.remove(&hash);

            if has_all {
                Self::process_available_hash(gossip, pending, hash).await?;
            }
        } else {
            pending.needed_layers.remove(&hash);
        }
        Ok(())
    }

    /// Get any needed requests.
    pub(super) async fn get_data_requests(
        &mut self,
        gossip: &Gossip<App>,
    ) -> Result<SmallVec<[DataRequest; REQUEST_COUNT]>> {
        let mut res = SmallVec::new();

        // First determine if we need to force sync from the beginning.
        match gossip.sync_mode {
            // State mode allows us to just grab the blocks we need.
            SyncMode::StateTransfer => (),
            // Check the archive value and sync from there.
            SyncMode::Archive => {
                // Add earlier blocks to archive if we know there are later blocks available.
                if let Some((next_needed, _)) = self.needed_blocks.first_key_value() {
                    let next_to_archive = gossip.kolme.get_next_to_archive().await;
                    if next_to_archive < *next_needed {
                        self.add_needed_block(gossip, next_to_archive, None).await?;
                    }
                }
            }
            SyncMode::BlockTransfer => {
                // If there are any blocks needed that are _later_ than our currently
                // expected next block, get the next block.
                if let Some((next_needed, _)) = self.needed_blocks.first_key_value() {
                    let next_chain = gossip.kolme.read().get_next_height();
                    if *next_needed != next_chain {
                        self.add_needed_block(gossip, next_chain, None).await?;
                    }
                }
            }
        }

        // Use a temporary array since we can't iterate and remove at the same time.
        let mut blocks_to_drop = SmallVec::<[BlockHeight; REQUEST_COUNT]>::new();

        for (height, waiting) in &mut self.needed_blocks {
            // We only want to process a single block at a time. If there are requests, then exit.
            if !res.is_empty() {
                break;
            }

            let height = *height;
            let label = DataLabel::Block(height);
            // Check if it got added to our store in the meanwhile
            if gossip.kolme.has_block(height).await? {
                blocks_to_drop.push(height);
                continue;
            }

            match waiting {
                WaitingBlock::Needed(status) => {
                    let request_new_peers = match status.should_request(gossip, label) {
                        ShouldRequest::DontRequest => {
                            continue;
                        }
                        ShouldRequest::RequestNoPeers => false,
                        ShouldRequest::RequestWithPeers => true,
                    };

                    res.push(DataRequest {
                        data: label,
                        current_peers: status.peers.clone(),
                        request_new_peers,
                    });
                }
                WaitingBlock::Received { block, peer } => {
                    // Determine if we're going to use block or state sync.
                    let do_block = match gossip.sync_mode {
                        SyncMode::BlockTransfer => true,
                        SyncMode::StateTransfer | SyncMode::Archive => {
                            let kolme = gossip.kolme.read();
                            kolme.get_next_height().next() == block.height()
                                && kolme.get_chain_version() == kolme.get_code_version()
                        }
                    };
                    if do_block {
                        if let Err(e) = gossip
                            .kolme
                            .add_block_with(block.clone(), gossip.data_load_validation)
                            .await
                        {
                            tracing::warn!(
                                "{}: error processing block {}: {e}",
                                gossip.local_display_name,
                                block.height()
                            );
                        }
                        blocks_to_drop.push(block.height());
                    } else {
                        let mut pending = PendingBlock {
                            block: block.clone(),
                            needed_layers: BTreeMap::new(),
                            pending_layers: HashMap::new(),
                            reverse_layers: HashMap::new(),
                        };

                        let inner = pending.block.0.message.as_inner();
                        for hash in [inner.framework_state, inner.app_state, inner.logs] {
                            if !gossip.kolme.has_merkle_hash(hash).await? {
                                pending
                                    .needed_layers
                                    .insert(hash, RequestStatus::new(*peer));
                                self.reverse_blocks.entry(hash).or_default().insert(height);
                            }
                        }

                        let res =
                            Self::get_block_data_requests(gossip, &mut pending, &mut res).await;
                        *waiting = WaitingBlock::Pending(pending);
                        res?;
                    }
                }
                WaitingBlock::Pending(pending) => {
                    Self::get_block_data_requests(gossip, pending, &mut res).await?;
                }
            }
        }
        for height in blocks_to_drop {
            self.needed_blocks.remove(&height);
        }
        Ok(res)
    }

    async fn get_block_data_requests(
        gossip: &Gossip<App>,
        pending: &mut PendingBlock<App::Message>,
        res: &mut SmallVec<[DataRequest; REQUEST_COUNT]>,
    ) -> Result<()> {
        let mut active_count = 0;
        let mut layers_to_drop = SmallVec::<[Sha256Hash; REQUEST_COUNT]>::new();
        for (hash, status) in &mut pending.needed_layers {
            if active_count >= REQUEST_COUNT {
                break;
            }
            let hash = *hash;
            let label = DataLabel::Merkle(hash);
            // Check if it got added to our store in the meanwhile
            if gossip.kolme.has_merkle_hash(hash).await? {
                layers_to_drop.push(hash);
                continue;
            }

            active_count += 1;

            let request_new_peers = match status.should_request(gossip, label) {
                ShouldRequest::DontRequest => {
                    continue;
                }
                ShouldRequest::RequestNoPeers => false,
                ShouldRequest::RequestWithPeers => true,
            };

            res.push(DataRequest {
                data: label,
                current_peers: status.peers.clone(),
                request_new_peers,
            });
        }

        for hash in layers_to_drop {
            pending.needed_layers.remove(&hash);
        }

        if pending.needed_layers.is_empty() {
            #[cfg(debug_assertions)]
            {
                debug_assert!(pending.pending_layers.is_empty());
                debug_assert!(pending.reverse_layers.is_empty());
                let inner = pending.block.0.message.as_inner();
                for hash in [inner.framework_state, inner.app_state, inner.logs] {
                    assert!(gossip.kolme.has_merkle_hash(hash).await?);
                }
            }

            gossip
                .kolme
                .add_block_with_state(pending.block.clone())
                .await?;
        }

        Ok(())
    }

    async fn process_available_hash(
        gossip: &Gossip<App>,
        pending: &mut PendingBlock<App::Message>,
        hash: Sha256Hash,
    ) -> Result<()> {
        if gossip.kolme.has_merkle_hash(hash).await? {
            // Awesome, we have the hash stored, we can drop it from pending and continue upstream.
            pending.pending_layers.remove(&hash);
        } else {
            // Get it from pending... if it's not there, we have a problem.
            match pending.pending_layers.entry(hash) {
                hash_map::Entry::Occupied(entry) => {
                    // Check if all the children are stored.
                    for child in &entry.get().children {
                        if !gossip.kolme.has_merkle_hash(*child).await? {
                            // Not all children available yet, so wait.
                            return Ok(());
                        }
                    }

                    // OK, we have all the children! Time to store it.
                    gossip.kolme.add_merkle_layer(hash, entry.get()).await?;

                    entry.remove_entry();
                }
                hash_map::Entry::Vacant(_) => {
                    debug_assert!(false);
                    return Ok(());
                }
            }
        }

        if let Some(parents) = pending.reverse_layers.get(&hash).cloned() {
            for parent in parents {
                Box::pin(Self::process_available_hash(gossip, pending, parent)).await?;
            }
            pending.reverse_layers.remove(&hash);
        }

        Ok(())
    }

    pub(super) fn add_layer_peer(&mut self, hash: Sha256Hash, peer: PeerId) {
        for height in self.get_heights_for_layer(hash) {
            if let Some(WaitingBlock::Pending(pending)) = self.needed_blocks.get_mut(&height) {
                if let Some(status) = pending.needed_layers.get_mut(&hash) {
                    status.add_peer(peer);
                }
            }
        }
    }

    pub(super) fn remove_layer_peer(&mut self, hash: Sha256Hash, peer: PeerId) {
        for height in self.get_heights_for_layer(hash) {
            if let Some(WaitingBlock::Pending(pending)) = self.needed_blocks.get_mut(&height) {
                if let Some(status) = pending.needed_layers.get_mut(&hash) {
                    status.remove_peer(peer);
                }
            }
        }
    }

    pub(super) async fn add_new_block(
        &mut self,
        gossip: &Gossip<App>,
        block: &Arc<SignedBlock<App::Message>>,
    ) {
        match self.add_needed_block(gossip, block.height(), None).await {
            Ok(()) => self.add_pending_block(gossip, block.clone(), None).await,
            Err(e) => {
                tracing::warn!("{}: add_new_block error: {e}", gossip.local_display_name);
            }
        }
    }

    pub(super) async fn add_report_block_height(
        &mut self,
        gossip: &Gossip<App>,
        ReportBlockHeight {
            next,
            peer,
            timestamp: _,
            latest_block: _,
        }: ReportBlockHeight,
    ) {
        tracing::debug!(
            "{}: got block height report message: peer=={peer}, next=={next}",
            gossip.local_display_name
        );

        let Some(height) = next.prev() else { return };

        // All sync preferences currently want to download latest blocks.
        // So add this to the needed blocks.
        if let Err(e) = self.add_needed_block(gossip, height, Some(peer)).await {
            tracing::warn!(
                "{}: got error while adding needed block from a block height report: {e}",
                gossip.local_display_name
            );
        }

        // FIXME do we need any of this anymore?
        // let our_next = self.kolme.read().get_next_height();
        // tracing::debug!(
        //     "{local_display_name}: Received ReportBlockHeight: {report:?}, our_next: {our_next}"
        // );
        // // Check if this peer has new blocks that we'd want to request.
        // if our_next < report.next {
        //     peers_with_blocks.try_send(report).ok();
        // }
        // let kolme = self.kolme.read();
        // let our_next = kolme.get_next_height();
        // let (next_to_sync, peer) = match self.get_next_to_sync(report_block_height, our_next).await
        // {
        //     Ok(None) => {
        //         tracing::debug!("{}: catch_up: no new node to sync", self.local_display_name);
        //         return;
        //     }
        //     Ok(Some(pair)) => pair,
        //     Err(e) => {
        //         tracing::error!(
        //             "{}: error calling get_next_to_sync: {e}",
        //             self.local_display_name
        //         );
        //         return;
        //     }
        // };

        // match self
        //     .state_sync
        //     .lock()
        //     .await
        //     .add_needed_block(self, next_to_sync, Some(peer))
        //     .await
        // {
        //     Ok(()) => self.trigger_state_sync.trigger(),
        //     Err(e) => tracing::error!(
        //         "{}: error adding needed block: {e}",
        //         self.local_display_name
        //     ),
        // }
    }
}

const REQUEST_COUNT: usize = 4;
const WARNING_PERIOD_SECS: u64 = 5;
