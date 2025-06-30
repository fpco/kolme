// ! Implementation of the state sync state machine.
//
// Due to the complexity of the topic, there's a dedicated doc for this on the docs
// site called "State sync implementation". Recommendation: read that before reading
// the code below.
use std::{
    collections::{hash_map, HashMap, HashSet, VecDeque},
    time::Instant,
};

use libp2p::PeerId;
use smallvec::SmallVec;

use crate::*;

/// Status of state syncing.
pub(super) struct StateSyncStatus<App: KolmeApp> {
    kolme: Kolme<App>,
    /// A queue of data requests we need to process.
    queue: VecDeque<PendingData>,
    /// Blocks we're interested in but haven't received any info on yet.
    needed_blocks: HashMap<BlockHeight, RequestStatus>,
    /// Blocks we have info on but still need to get the full Merkle data for.
    pending_blocks: HashMap<BlockHeight, Arc<SignedBlock<App::Message>>>,
    /// A reverse map of missing top-level hashes to pending blocks.
    ///
    /// This allows us to efficiently discover which blocks should be
    /// checked when a new Merkle contents is fully discovered.
    reverse_blocks: HashMap<Sha256Hash, HashSet<BlockHeight>>,
    /// Layers we're interested in but haven't received any info on yet.
    needed_layers: HashMap<Sha256Hash, RequestStatus>,
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

#[derive(Clone, Copy, Debug)]
enum PendingData {
    Block(BlockHeight),
    Merkle(Sha256Hash),
}

#[derive(Debug)]
pub(super) enum DataRequest {
    GetBlock {
        height: BlockHeight,
        current_peers: SmallVec<[PeerId; REQUEST_COUNT]>,
        request_new_peers: bool,
    },
    GetMerkle {
        hash: Sha256Hash,
        current_peers: SmallVec<[PeerId; REQUEST_COUNT]>,
        request_new_peers: bool,
    },
}

/// Status of a request for data.
#[derive(Debug)]
struct RequestStatus {
    /// When was the last time we tried with a new peer? If it's too long
    /// ago, we need to find new peers to query.
    last_new_peer: Instant,
    /// Peers we can query for the data.
    peers: SmallVec<[PeerId; REQUEST_COUNT]>,
}

impl RequestStatus {
    fn new(peer: Option<PeerId>) -> Self {
        RequestStatus {
            last_new_peer: Instant::now(),
            peers: peer.into_iter().collect(),
        }
    }

    fn need_new_peers(&self) -> bool {
        self.last_new_peer.elapsed().as_secs() > MAX_NEW_PEER_DELAY
    }

    fn add_peer(&mut self, peer: PeerId) {
        if self.peers.len() >= REQUEST_COUNT {
            debug_assert!(self.peers.len() == REQUEST_COUNT);
            self.peers.remove(0);
        }
        self.peers.push(peer);
    }
}

impl<App: KolmeApp> StateSyncStatus<App> {
    pub(super) fn new(kolme: Kolme<App>) -> Self {
        StateSyncStatus {
            kolme,
            queue: VecDeque::new(),
            needed_blocks: HashMap::new(),
            pending_blocks: HashMap::new(),
            reverse_blocks: HashMap::new(),
            needed_layers: HashMap::new(),
            pending_layers: HashMap::new(),
            reverse_layers: HashMap::new(),
        }
    }

    pub(super) async fn add_needed_block(
        &mut self,
        height: BlockHeight,
        peer: Option<PeerId>,
    ) -> Result<()> {
        if !self.kolme.has_block(height).await? {
            self.needed_blocks.insert(height, RequestStatus::new(peer));
            self.queue.push_front(PendingData::Block(height));
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
                    self.queue.push_back(PendingData::Merkle(hash));
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
                        .insert(*child, RequestStatus::new(Some(peer)));
                    self.queue.push_back(PendingData::Merkle(*child));
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

    pub(super) async fn get_requests(&mut self) -> Result<SmallVec<[DataRequest; REQUEST_COUNT]>> {
        // How many items have we processed? We use this to ensure we don't cycle back.
        let mut processed = 0;
        // And how many items were in the queue in the first place?
        let total = self.queue.len();
        // Result value
        let mut res = SmallVec::new();

        while res.len() < REQUEST_COUNT && processed < total {
            let Some(item) = self.queue.pop_front() else {
                break;
            };

            processed += 1;

            match self.make_data_request(item).await {
                Ok(None) => {
                    // Don't need to process this item anymore, don't add it back.
                }
                Ok(Some(request)) => {
                    // Still an active item, add the request and push to the back of the queue.
                    self.queue.push_back(item);
                    res.push(request);
                }
                Err(e) => {
                    // Some error... maybe we should just report here and continue. Instead, we'll raise for now.
                    self.queue.push_back(item);
                    return Err(e);
                }
            }
        }

        Ok(res)
    }

    /// Returns None if the data request is no longer needed.
    async fn make_data_request(&mut self, item: PendingData) -> Result<Option<DataRequest>> {
        match item {
            PendingData::Block(height) => {
                // We either got the block fully in the database, or it's already pending.
                // Either way, nothing else to be done.
                if self.kolme.has_block(height).await? || self.pending_blocks.contains_key(&height)
                {
                    return Ok(None);
                }
                match self.needed_blocks.get(&height) {
                    Some(status) => Ok(Some(DataRequest::GetBlock {
                        height,
                        current_peers: status.peers.clone(),
                        request_new_peers: status.need_new_peers(),
                    })),
                    None => {
                        // This shouldn't happen. If it's not in needed, it should be in pending.
                        debug_assert!(false);
                        Ok(None)
                    }
                }
            }
            PendingData::Merkle(hash) => {
                // Either already in the database or we have the layer
                // pending the children, either way, no need to request it anymore.
                // However, we should process it in case it was added to the database
                // elsewhere.
                if self.kolme.has_merkle_hash(hash).await?
                    || self.pending_layers.contains_key(&hash)
                {
                    self.process_available_hash(hash).await?;
                    return Ok(None);
                }

                match self.needed_layers.get(&hash) {
                    Some(status) => Ok(Some(DataRequest::GetMerkle {
                        hash,
                        current_peers: status.peers.clone(),
                        request_new_peers: status.need_new_peers(),
                    })),
                    None => {
                        // This shouldn't happen. If it's not in needed, it should be in pending.
                        debug_assert!(false);
                        Ok(None)
                    }
                }
            }
        }
    }

    async fn process_available_block(&mut self, height: BlockHeight) -> Result<()> {
        if self.kolme.has_block(height).await? {
            // Nothing to be done
            self.pending_blocks.remove(&height);
            Ok(())
        } else {
            // Get it from pending... if it's not there, we have a problem.
            match self.pending_blocks.entry(height) {
                hash_map::Entry::Occupied(entry) => {
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
                hash_map::Entry::Vacant(_) => {
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
                hash_map::Entry::Occupied(entry) => {
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
                hash_map::Entry::Vacant(_) => {
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

    pub(super) async fn add_needed_layer(
        &mut self,
        hash: Sha256Hash,
        peer: Option<PeerId>,
    ) -> Result<()> {
        if !self.kolme.has_merkle_hash(hash).await? {
            self.needed_layers.insert(hash, RequestStatus::new(peer));
            self.queue.push_back(PendingData::Merkle(hash));
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
const MAX_NEW_PEER_DELAY: u64 = 5;
