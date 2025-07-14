// ! Implementation of the state sync state machine.
//
// Due to the complexity of the topic, there's a dedicated doc for this on the docs
// site called "State sync implementation". Recommendation: read that before reading
// the code below.
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    time::Instant,
};

use gossip::{ReportBlockHeight, SyncPreference, Trigger};
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

enum WaitingBlock<AppMessage> {
    /// We haven't received the basic block info yet.
    Needed(RequestStatus),
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
        if let Err(e) = self.add_pending_block_inner(gossip, block, peer).await {
            tracing::warn!(
                "{}: add_pending_block failed: {e}",
                gossip.local_display_name
            );
        }
    }

    async fn add_pending_block_inner(
        &mut self,
        gossip: &Gossip<App>,
        block: Arc<SignedBlock<App::Message>>,
        peer: Option<PeerId>,
    ) -> Result<()> {
        let height = block.height();

        if gossip.kolme.has_block(height).await? {
            self.needed_blocks.remove(&height);
            return Ok(());
        }

        let Some(waiting) = self.needed_blocks.get_mut(&height) else {
            return Ok(());
        };

        match waiting {
            // We needed this block, so let's do it.
            WaitingBlock::Needed(_) => (),
            // We already got it, nothing to do.
            WaitingBlock::Pending(_) => return Ok(()),
        }

        let do_block = match gossip.sync_mode {
            // If we have the prior block and we have the right code version, still
            // do a block sync.
            // FIXME reconsider the StateTransferForUpgrade entirely, maybe it's too wonky. This is not the right implementation regardless.
            SyncMode::StateTransfer | SyncMode::StateTransferForUpgrade => {
                match height.prev() {
                    // We're at the starting block, so just confirm that we have
                    // the same code version as the genesis
                    None => {
                        &gossip.kolme.get_genesis_info().version == gossip.kolme.get_code_version()
                    }
                    Some(needed) => {
                        match gossip.kolme.load_block(needed).await {
                            Ok(block) => {
                                block.framework_state.get_chain_version()
                                    == gossip.kolme.get_code_version()
                            }
                            // For any errors, just go back to block sync
                            Err(_) => false,
                        }
                    }
                }
            }
            SyncMode::BlockTransfer => {
                // Block transfer mode: we never allow a state transfer
                true
            }
        };

        if do_block {
            // FIXME handle all the logic around requesting older blocks if needed
            gossip.kolme.add_block(block).await?;
        } else {
            let mut pending = PendingBlock {
                block,
                needed_layers: BTreeMap::new(),
                pending_layers: HashMap::new(),
                reverse_layers: HashMap::new(),
            };

            let inner = pending.block.0.message.as_inner();
            for hash in [inner.framework_state, inner.app_state, inner.logs] {
                if !gossip.kolme.has_merkle_hash(hash).await? {
                    pending.needed_layers.insert(hash, RequestStatus::new(peer));
                    self.reverse_blocks.entry(hash).or_default().insert(height);
                }
            }

            let has_all = pending.needed_layers.is_empty();
            *waiting = WaitingBlock::Pending(pending);
            if has_all {
                self.process_available_block(gossip, height).await?;
            }
        }

        Ok(())
    }

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
            WaitingBlock::Needed(_) => return Ok(()),
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
                self.process_available_hash(gossip, hash).await?;
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

        if let SyncPreference::FromBeginning = gossip.sync_preference {
            // Do an archive check to see if we need to sync some additional
            // blocks.
            todo!()

            // /// Based on current state info and the sync preference, determine the next block to sync.
            // async fn get_next_to_sync(
            //     &self,
            //     report_block_height: Option<ReportBlockHeight>,
            //     our_next: BlockHeight,
            // ) -> Result<Option<(BlockHeight, PeerId)>> {
            //     let (do_latest, do_archive) = match self.sync_preference {
            //         SyncPreference::Latest => (true, false),
            //         SyncPreference::FromBeginning => (false, true),
            //         SyncPreference::LatestThenBeginning => (true, true),
            //     };

            //     let ReportBlockHeight {
            //         next: their_next,
            //         peer,
            //         timestamp: _,
            //         latest_block: _,
            //     } = match report_block_height {
            //         Some(report) => report,
            //         None => {
            //             tracing::debug!(
            //                 "{}: get_next_to_sync found no ReportBlockHeight",
            //                 self.local_display_name
            //             );
            //             return Ok(None);
            //         }
            //     };

            //     tracing::debug!(
            //         "{}: In get_next_to_sync, their_next=={their_next}, peer=={peer}, our_next=={our_next}",
            //         self.local_display_name
            //     );

            //     let their_highest = match their_next.prev() {
            //         None => return Ok(None),
            //         Some(highest) => highest,
            //     };

            //     // If we're checking for latest, see if there's anything new for us.
            //     if do_latest && their_highest >= our_next {
            //         return Ok(Some((our_next, peer)));
            //     }

            //     // Now see if we have any archive work
            //     if do_archive {
            //         let next_to_archive = self.kolme.get_next_to_archive().await;
            //         if next_to_archive <= their_highest {
            //             // We don't know for sure that the peer will have an older
            //             // block, but we can request and, if it doesn't, we'll remove
            //             // the peer from the list of peers to query in the future.
            //             return Ok(Some((next_to_archive, peer)));
            //         }
            //     }

            //     // We're totally caught up
            //     Ok(None)
            // }
        }

        // Use a temporary array since we can't iterate and remove at the same time.
        let mut blocks_to_drop = SmallVec::<[BlockHeight; REQUEST_COUNT]>::new();

        // Counts how many active requests we have going. This will include requests
        // that we've made so recently that they are not added to res.
        let mut active_count = 0;

        for (height, waiting) in &mut self.needed_blocks {
            let height = *height;
            let label = DataLabel::Block(height);
            // Check if it got added to our store in the meanwhile
            if gossip.kolme.has_block(height).await? {
                blocks_to_drop.push(height);
                continue;
            }

            match waiting {
                WaitingBlock::Needed(status) => {
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
                WaitingBlock::Pending(pending) => {
                    Self::get_block_data_requests(gossip, pending, &mut res, &mut active_count)
                        .await?;
                    if active_count >= REQUEST_COUNT {
                        break;
                    }
                }
            }
        }
        for height in blocks_to_drop {
            self.needed_blocks.remove(&height);
        }
        Ok(res)
        // // Next, if we don't have an active block, put in the next needed block.
        // // Prune any blocks in the list that don't need to be there.
        // while self.active_block.is_none() {
        //     let Some((height, _)) = self.needed_blocks.first_key_value() else {
        //         break;
        //     };
        //     let height = *height;
        //     if self.pending_blocks.contains_key(&height) || gossip.kolme.has_block(height).await? {
        //         self.needed_blocks.remove(&height);
        //     }
        //     self.active_block = Some(height);
        // }

        // // If there's an active block, determine if we should send back a data request or not.
        // let Some(height) = self.active_block else {
        //     return Ok(None);
        // };
        // let Some((status, _)) = self.needed_blocks.get_mut(&height) else {
        //     // This should never happen. We'll ignore in prod, but want
        //     // our test suites to fail.
        //     debug_assert!(false);
        //     return Ok(None);
        // };
    }

    async fn get_block_data_requests(
        gossip: &Gossip<App>,
        pending: &mut PendingBlock<App::Message>,
        res: &mut SmallVec<[DataRequest; REQUEST_COUNT]>,
        active_count: &mut usize,
    ) -> Result<()> {
        let mut layers_to_drop = SmallVec::<[Sha256Hash; REQUEST_COUNT]>::new();
        for (hash, status) in &mut pending.needed_layers {
            if *active_count >= REQUEST_COUNT {
                break;
            }
            let hash = *hash;
            let label = DataLabel::Merkle(hash);
            // Check if it got added to our store in the meanwhile
            if gossip.kolme.has_merkle_hash(hash).await? {
                layers_to_drop.push(hash);
                continue;
            }

            *active_count += 1;

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

        Ok(())
    }

    // pub(super) async fn get_layer_requests(
    //     &mut self,
    //     gossip: &Gossip<App>,
    // ) -> Result<SmallVec<[DataRequest<Sha256Hash>; REQUEST_COUNT]>> {
    //     todo!()
    // // First, clear out an existing layer requests that have already been fulfilled.
    // for (idx, hash) in self.active_layers.clone().into_iter().enumerate().rev() {
    //     if self.pending_layers.contains_key(&hash) || gossip.kolme.has_merkle_hash(hash).await?
    //     {
    //         self.active_layers.remove(idx);
    //     }
    // }

    // // Next, while we haven't filled our active queue, keep filling in more
    // // from the needed layers.
    // let mut to_remove = SmallVec::<[Sha256Hash; REQUEST_COUNT]>::new();
    // for hash in self.needed_layers.keys() {
    //     if self.active_layers.len() >= REQUEST_COUNT {
    //         break;
    //     }
    //     if self.pending_layers.contains_key(hash) || gossip.kolme.has_merkle_hash(*hash).await?
    //     {
    //         to_remove.push(*hash);
    //     } else {
    //         self.active_layers.push(*hash);
    //     }
    // }
    // for hash in to_remove {
    //     self.needed_layers.remove(&hash);
    // }

    // let mut res = SmallVec::new();

    // for hash in &self.active_layers {
    //     let Some(status) = self.needed_layers.get_mut(hash) else {
    //         // This should never happen. We'll ignore in prod, but want
    //         // our test suites to fail.
    //         debug_assert!(false);
    //         continue;
    //     };

    //     let request_new_peers = match status.should_request(DataLabel::Merkle(*hash)) {
    //         ShouldRequest::DontRequest => continue,
    //         ShouldRequest::RequestNoPeers => false,
    //         ShouldRequest::RequestWithPeers => true,
    //     };

    //     res.push(DataRequest {
    //         data: *hash,
    //         current_peers: status.peers.clone(),
    //         request_new_peers,
    //     });
    // }

    // Ok(res)
    // }

    async fn process_available_block(
        &mut self,
        gossip: &Gossip<App>,
        height: BlockHeight,
    ) -> Result<()> {
        todo!()
        // if gossip.kolme.has_block(height).await? {
        //     // Nothing to be done
        //     self.pending_blocks.remove(&height);
        //     Ok(())
        // } else {
        //     // Get it from pending... if it's not there, we have a problem.
        //     match self.pending_blocks.entry(height) {
        //         btree_map::Entry::Occupied(entry) => {
        //             let signed_block = entry.get().clone();

        //             let block = signed_block.0.message.as_inner();
        //             for hash in [block.framework_state, block.app_state, block.logs] {
        //                 if !gossip.kolme.has_merkle_hash(hash).await? {
        //                     return Ok(());
        //                 }
        //             }

        //             gossip.kolme.add_block_with_state(signed_block).await?;
        //             entry.remove_entry();
        //             Ok(())
        //         }
        //         btree_map::Entry::Vacant(_) => {
        //             debug_assert!(false);
        //             Ok(())
        //         }
        //     }
        // }
    }

    async fn process_available_hash(
        &mut self,
        gossip: &Gossip<App>,
        hash: Sha256Hash,
    ) -> Result<()> {
        todo!()
        // if gossip.kolme.has_merkle_hash(hash).await? {
        //     // Awesome, we have the hash stored, we can drop it from pending and continue upstream.
        //     self.pending_layers.remove(&hash);
        // } else {
        //     // Get it from pending... if it's not there, we have a problem.
        //     match self.pending_layers.entry(hash) {
        //         btree_map::Entry::Occupied(entry) => {
        //             // Check if all the children are stored.
        //             for child in &entry.get().children {
        //                 if !gossip.kolme.has_merkle_hash(*child).await? {
        //                     // Not all children available yet, so wait.
        //                     return Ok(());
        //                 }
        //             }

        //             // OK, we have all the children! Time to store it.
        //             gossip.kolme.add_merkle_layer(hash, entry.get()).await?;

        //             entry.remove_entry();
        //         }
        //         btree_map::Entry::Vacant(_) => {
        //             debug_assert!(false);
        //             return Ok(());
        //         }
        //     }
        // }

        // // OK, we've discovered that either this layer was already in the store,
        // // or we just wrote it there. Either way, now go check all the reverse dependencies.
        // if let Some(heights) = self.reverse_blocks.get(&hash).cloned() {
        //     for height in heights {
        //         self.process_available_block(gossip, height).await?;
        //     }
        //     self.reverse_blocks.remove(&hash);
        // }

        // if let Some(parents) = self.reverse_layers.get(&hash).cloned() {
        //     for parent in parents {
        //         Box::pin(self.process_available_hash(gossip, parent)).await?;
        //     }
        //     self.reverse_layers.remove(&hash);
        // }

        // Ok(())
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
