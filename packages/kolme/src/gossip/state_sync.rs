// ! Implementation of the state sync state machine.
//
// Due to the complexity of the topic, there's a dedicated doc for this on the docs
// site called "State sync implementation". Recommendation: read that before reading
// the code below.
use std::{
    collections::{btree_map, HashMap, HashSet},
    time::Instant,
};

use libp2p::PeerId;
use smallvec::SmallVec;

use crate::*;

/// Status of state syncing.
pub(super) struct StateSyncStatus<App: KolmeApp> {
    kolme: Kolme<App>,
    /// Blocks we're interested in but haven't received any info on yet.
    needed_blocks: BTreeMap<BlockHeight, RequestStatus>,
    /// The block we are currently in the process of requesting, if any.
    active_block: Option<BlockHeight>,
    /// Blocks we have info on but still need to get the full Merkle data for.
    pending_blocks: BTreeMap<BlockHeight, Arc<SignedBlock<App::Message>>>,
    /// A reverse map of missing top-level hashes to pending blocks.
    ///
    /// This allows us to efficiently discover which blocks should be
    /// checked when a new Merkle contents is fully discovered.
    reverse_blocks: HashMap<Sha256Hash, HashSet<BlockHeight>>,
    /// Layers we're interested in but haven't received any info on yet.
    needed_layers: BTreeMap<Sha256Hash, RequestStatus>,
    /// Layers that we are actively requesting right now.
    active_layers: SmallVec<[Sha256Hash; REQUEST_COUNT]>,
    /// Merkle layers that don't yet have the full content for the children.
    pending_layers: BTreeMap<Sha256Hash, MerkleLayerContents>,
    /// A reverse map for pending layers: parents waiting on children.
    ///
    /// Every time we discover a layer with unknown children, we fill in
    /// both pending_layers and this field. When we complete a layer,
    /// we can look up the parents in this map and see if they were completed
    /// too.
    reverse_layers: HashMap<Sha256Hash, HashSet<Sha256Hash>>,
}

#[derive(Debug)]
pub(super) struct DataRequest<T> {
    pub(super) data: T,
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
}

impl RequestStatus {
    fn new(peer: Option<PeerId>) -> Self {
        RequestStatus {
            last_sent: None,
            peers: peer.into_iter().collect(),
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
    /// should be made.
    fn should_request(&mut self) -> ShouldRequest {
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
        match res {
            ShouldRequest::DontRequest => (),
            ShouldRequest::RequestNoPeers | ShouldRequest::RequestWithPeers => {
                self.last_sent = Some(Instant::now())
            }
        }
        res
    }
}

impl<App: KolmeApp> StateSyncStatus<App> {
    pub(super) fn new(kolme: Kolme<App>) -> Self {
        StateSyncStatus {
            kolme,
            needed_blocks: BTreeMap::new(),
            active_block: None,
            pending_blocks: BTreeMap::new(),
            reverse_blocks: HashMap::new(),
            needed_layers: BTreeMap::new(),
            active_layers: SmallVec::new(),
            pending_layers: BTreeMap::new(),
            reverse_layers: HashMap::new(),
        }
    }

    pub(super) async fn add_needed_block(
        &mut self,
        height: BlockHeight,
        peer: Option<PeerId>,
    ) -> Result<()> {
        if !self.kolme.has_block(height).await? && !self.pending_blocks.contains_key(&height) {
            self.needed_blocks
                .entry(height)
                .or_insert(RequestStatus::new(peer));
        }
        Ok(())
    }

    pub(super) fn add_block_peer(&mut self, height: BlockHeight, peer: PeerId) {
        if let Some(status) = self.needed_blocks.get_mut(&height) {
            status.add_peer(peer);
        }
    }

    pub(super) async fn add_pending_block(
        &mut self,
        block: Arc<SignedBlock<App::Message>>,
        peer: PeerId,
    ) -> Result<()> {
        let height = block.height();
        if !self.needed_blocks.contains_key(&height) || self.pending_blocks.contains_key(&height) {
            return Ok(());
        }
        if !self.kolme.has_block(block.height()).await? {
            let inner = block.0.message.as_inner();
            let mut has_all = true;
            for hash in [inner.framework_state, inner.app_state, inner.logs] {
                if !self.kolme.has_merkle_hash(hash).await? {
                    has_all = false;
                    self.reverse_blocks.entry(hash).or_default().insert(height);
                    self.needed_layers
                        .insert(hash, RequestStatus::new(Some(peer)));
                }
            }

            self.pending_blocks.insert(height, block);
            if has_all {
                self.process_available_block(height).await?;
            }
        }
        self.needed_blocks.remove(&height);
        Ok(())
    }

    pub(super) async fn add_merkle_layer(
        &mut self,
        hash: Sha256Hash,
        layer: MerkleLayerContents,
        peer: PeerId,
    ) -> Result<()> {
        if !self.needed_layers.contains_key(&hash) || self.pending_layers.contains_key(&hash) {
            return Ok(());
        }
        if !self.kolme.has_merkle_hash(hash).await? {
            let mut has_all = true;
            for child in &layer.children {
                if !self.kolme.has_merkle_hash(*child).await? {
                    has_all = false;
                    self.reverse_layers.entry(*child).or_default().insert(hash);
                    self.needed_layers
                        .entry(*child)
                        .or_insert(RequestStatus::new(Some(peer)));
                }
            }

            self.pending_layers.insert(hash, layer);

            if has_all {
                self.process_available_hash(hash).await?;
            }
        }
        self.needed_layers.remove(&hash);
        Ok(())
    }

    /// Get any needed block data request, if relevant.
    pub(super) async fn get_block_request(&mut self) -> Result<Option<DataRequest<BlockHeight>>> {
        // First, clear out an existing block request if it's already been fulfilled.
        if let Some(height) = self.active_block {
            if self.pending_blocks.contains_key(&height) || self.kolme.has_block(height).await? {
                self.active_block = None;
            }
        }

        // Next, if we don't have an active block, put in the next needed block.
        // Prune any blocks in the list that don't need to be there.
        while self.active_block.is_none() {
            let Some((height, _)) = self.needed_blocks.first_key_value() else {
                break;
            };
            let height = *height;
            if self.pending_blocks.contains_key(&height) || self.kolme.has_block(height).await? {
                self.needed_blocks.remove(&height);
            }
            self.active_block = Some(height);
        }

        // If there's an active block, determine if we should send back a data request or not.
        let Some(height) = self.active_block else {
            return Ok(None);
        };
        let Some(status) = self.needed_blocks.get_mut(&height) else {
            // This should never happen. We'll ignore in prod, but want
            // our test suites to fail.
            debug_assert!(false);
            return Ok(None);
        };

        let request_new_peers = match status.should_request() {
            ShouldRequest::DontRequest => {
                return Ok(None);
            }
            ShouldRequest::RequestNoPeers => false,
            ShouldRequest::RequestWithPeers => true,
        };

        Ok(Some(DataRequest {
            data: height,
            current_peers: status.peers.clone(),
            request_new_peers,
        }))
    }

    pub(super) async fn get_layer_requests(
        &mut self,
    ) -> Result<SmallVec<[DataRequest<Sha256Hash>; REQUEST_COUNT]>> {
        // First, clear out an existing layer requests that have already been fulfilled.
        for (idx, hash) in self.active_layers.clone().into_iter().enumerate().rev() {
            if self.pending_layers.contains_key(&hash) || self.kolme.has_merkle_hash(hash).await? {
                self.active_layers.remove(idx);
            }
        }

        // Next, while we haven't filled our active queue, keep filling in more
        // from the needed layers.
        let mut to_remove = SmallVec::<[Sha256Hash; REQUEST_COUNT]>::new();
        for hash in self.needed_layers.keys() {
            if self.active_layers.len() >= REQUEST_COUNT {
                break;
            }
            if self.pending_layers.contains_key(hash) || self.kolme.has_merkle_hash(*hash).await? {
                to_remove.push(*hash);
            } else {
                self.active_layers.push(*hash);
            }
        }
        for hash in to_remove {
            self.needed_layers.remove(&hash);
        }

        let mut res = SmallVec::new();

        for hash in &self.active_layers {
            let Some(status) = self.needed_layers.get_mut(hash) else {
                // This should never happen. We'll ignore in prod, but want
                // our test suites to fail.
                debug_assert!(false);
                continue;
            };

            let request_new_peers = match status.should_request() {
                ShouldRequest::DontRequest => continue,
                ShouldRequest::RequestNoPeers => false,
                ShouldRequest::RequestWithPeers => true,
            };

            res.push(DataRequest {
                data: *hash,
                current_peers: status.peers.clone(),
                request_new_peers,
            });
        }

        Ok(res)
    }

    async fn process_available_block(&mut self, height: BlockHeight) -> Result<()> {
        if self.kolme.has_block(height).await? {
            // Nothing to be done
            self.pending_blocks.remove(&height);
            Ok(())
        } else {
            // Get it from pending... if it's not there, we have a problem.
            match self.pending_blocks.entry(height) {
                btree_map::Entry::Occupied(entry) => {
                    let signed_block = entry.get().clone();

                    let block = signed_block.0.message.as_inner();
                    for hash in [block.framework_state, block.app_state, block.logs] {
                        if !self.kolme.has_merkle_hash(hash).await? {
                            return Ok(());
                        }
                    }

                    self.kolme.add_block_with_state(signed_block).await?;
                    entry.remove_entry();
                    Ok(())
                }
                btree_map::Entry::Vacant(_) => {
                    debug_assert!(false);
                    Ok(())
                }
            }
        }
    }

    async fn process_available_hash(&mut self, hash: Sha256Hash) -> Result<()> {
        if self.kolme.has_merkle_hash(hash).await? {
            // Awesome, we have the hash stored, we can drop it from pending and continue upstream.
            self.pending_layers.remove(&hash);
        } else {
            // Get it from pending... if it's not there, we have a problem.
            match self.pending_layers.entry(hash) {
                btree_map::Entry::Occupied(entry) => {
                    // Check if all the children are stored.
                    for child in &entry.get().children {
                        if !self.kolme.has_merkle_hash(*child).await? {
                            // Not all children available yet, so wait.
                            return Ok(());
                        }
                    }

                    // OK, we have all the children! Time to store it.
                    self.kolme.add_merkle_layer(hash, entry.get()).await?;

                    entry.remove_entry();
                }
                btree_map::Entry::Vacant(_) => {
                    debug_assert!(false);
                    return Ok(());
                }
            }
        }

        // OK, we've discovered that either this layer was already in the store,
        // or we just wrote it there. Either way, now go check all the reverse dependencies.
        if let Some(heights) = self.reverse_blocks.get(&hash).cloned() {
            for height in heights {
                self.process_available_block(height).await?;
            }
            self.reverse_blocks.remove(&hash);
        }

        if let Some(parents) = self.reverse_layers.get(&hash).cloned() {
            for parent in parents {
                Box::pin(self.process_available_hash(parent)).await?;
            }
            self.reverse_layers.remove(&hash);
        }

        Ok(())
    }

    pub(super) fn add_layer_peer(&mut self, hash: Sha256Hash, peer: PeerId) {
        if let Some(status) = self.needed_layers.get_mut(&hash) {
            status.add_peer(peer);
        }
    }
}

const REQUEST_COUNT: usize = 4;
