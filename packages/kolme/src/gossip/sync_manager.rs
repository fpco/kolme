// ! Implementation of the sync manager state machine.
//
// Due to the complexity of the topic, there's a dedicated doc for this on the docs
// site called "Sync manager". Recommendation: read that before reading
// the code below.
use std::{
    collections::{hash_map, HashMap, HashSet},
    fmt::Display,
    time::Instant,
};

use gossip::Trigger;
use smallvec::SmallVec;
use utils::trigger::TriggerSubscriber;

use crate::*;

/// Status of state syncing.
pub(super) struct SyncManager<App: KolmeApp> {
    /// Blocks we're currently waiting on.
    needed_blocks: BTreeMap<BlockHeight, WaitingBlock<App>>,
    /// Trigger [Gossip] to recheck sync requests.
    trigger: Trigger,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum WaitingBlock<App: KolmeApp> {
    /// We haven't received the basic block info yet.
    Needed(RequestStatus),
    /// We have the raw block, but haven't processed it yet.
    Received(Arc<SignedBlock<App::Message>>),
    /// We've received the block info, but now need to download the state layers.
    Pending(PendingBlock<App>),
}

#[derive(Debug)]
struct PendingBlock<App: KolmeApp> {
    block: Arc<SignedBlock<App::Message>>,
    /// Layers we're interested in but haven't received any info on yet.
    needed_layers: BTreeMap<Sha256Hash, RequestStatus>,
    /// Merkle layers that don't yet have the full content for the children.
    pending_layers: HashMap<Sha256Hash, Arc<MerkleLayerContents>>,
    /// A reverse map for pending layers: parents waiting on children.
    ///
    /// Every time we discover a layer with unknown children, we fill in
    /// both pending_layers and this field. When we complete a layer,
    /// we can look up the parents in this map and see if they were completed
    /// too.
    reverse_layers: HashMap<Sha256Hash, HashSet<Sha256Hash>>,
}

/// Status of a request for data.
#[derive(Debug)]
struct RequestStatus {
    /// When was the last time we made a request?
    ///
    /// This starts off as [None]. The first time we make a request, we use the original peer only. Thereafter, we always request new peers as well.
    last_sent: Option<Instant>,
    /// Last time we updated the warning state.
    ///
    /// Warning state is to track how long a request has been unanswered. We update it (1) the first time we make a request and (2) every time we print out a warning.
    warning_updated: Option<Instant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DataRequest {
    Block(BlockHeight),
    Merkle(Sha256Hash),
}

impl Display for DataRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DataRequest::Block(height) => write!(f, "block {height}"),
            DataRequest::Merkle(hash) => write!(f, "merkle layer {hash}"),
        }
    }
}

impl RequestStatus {
    fn new() -> Self {
        RequestStatus {
            last_sent: None,
            warning_updated: None,
        }
    }

    /// Check if we should try this request again.
    ///
    /// This will automatically update the [last_sent] field if a request
    /// should be made. It's also responsible for printing warnings for
    /// requests that are taking too long.
    fn should_request<App: KolmeApp>(&mut self, gossip: &Gossip<App>, label: DataRequest) -> bool {
        let res = match self.last_sent {
            None => true,
            Some(last_sent) => last_sent.elapsed().as_secs() >= 2,
        };

        let now = Instant::now();
        match self.warning_updated {
            None => self.warning_updated = Some(now),
            Some(warning_updated) => {
                if now.duration_since(warning_updated) >= gossip.warning_period {
                    tracing::warn!(
                        "{}: still waiting on request for {label}",
                        gossip.local_display_name
                    );
                    self.warning_updated = Some(now);
                }
            }
        }

        if res {
            self.last_sent = Some(Instant::now())
        }
        res
    }
}

impl<App: KolmeApp> Default for SyncManager<App> {
    fn default() -> Self {
        SyncManager {
            needed_blocks: BTreeMap::new(),
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
    ) -> Result<()> {
        if gossip.kolme.has_block(height).await? || self.needed_blocks.contains_key(&height) {
            return Ok(());
        }

        self.needed_blocks
            .insert(height, WaitingBlock::Needed(RequestStatus::new()));
        self.trigger.trigger();
        Ok(())
    }

    pub(super) async fn add_pending_block(
        &mut self,
        gossip: &Gossip<App>,
        block: Arc<SignedBlock<App::Message>>,
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
                *waiting = WaitingBlock::Received(block);
                self.trigger.trigger();
            }
            // We already got it, nothing to do.
            WaitingBlock::Received { .. } | WaitingBlock::Pending(_) => (),
        }
    }

    pub(super) async fn add_merkle_layer(
        &mut self,
        gossip: &Gossip<App>,
        hash: Sha256Hash,
        layer: Arc<MerkleLayerContents>,
    ) -> Result<()> {
        let Some(mut entry) = self.needed_blocks.first_entry() else {
            return Ok(());
        };

        let pending = match entry.get_mut() {
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
                    pending
                        .needed_layers
                        .entry(*child)
                        .or_insert(RequestStatus::new());
                }
            }

            pending.pending_layers.insert(hash, layer);
            pending.needed_layers.remove(&hash);

            if has_all {
                Self::process_available_hash(gossip, pending, hash).await?;
            }
        } else {
            pending.needed_layers.remove(&hash);
        }
        self.trigger.trigger();
        Ok(())
    }

    /// Get any needed requests.
    pub(super) async fn get_data_requests(
        &mut self,
        gossip: &Gossip<App>,
    ) -> Result<SmallVec<[DataRequest; DEFAULT_REQUEST_COUNT]>> {
        // We need to make sure that we are correctly performing
        // requests in order. That means that, in some cases, we'll
        // need to insert earlier block requests so that we fill in
        // gaps in the chain. Also, in some cases, we'll remove
        // blocks from the needed list.
        //
        // To account for this, we'll perform operations in an infinite loop.
        // That loop will exit if the needed blocks is empty. It will also
        // take responsibility for adding missing block requests and removing
        // unnecessary ones.
        //
        // Without this approach, it's possible to (1) remove an out-of-date block
        // request and then (2) proceed to a block request we're not yet ready for.
        loop {
            self.add_missing_needed_blocks(gossip).await?;
            let Some(mut entry) = self.needed_blocks.first_entry() else {
                break Ok(SmallVec::new());
            };

            match Self::get_data_requests_for(gossip, *entry.key(), entry.get_mut()).await? {
                None => {
                    self.needed_blocks.pop_first();
                }
                Some(reqs) => break Ok(reqs),
            }
        }
    }

    async fn add_missing_needed_blocks(&mut self, gossip: &Gossip<App>) -> Result<()> {
        // First determine if we need to force sync from the beginning.
        match gossip.sync_mode {
            // State mode allows us to just grab the blocks we need.
            SyncMode::StateTransfer => (),
            // Check the archive value and sync from there.
            SyncMode::Archive => {
                // Add earlier blocks to archive if we know there are later blocks available.
                if let Some((next_needed, _)) = self.needed_blocks.first_key_value() {
                    let next_to_archive = gossip.kolme.get_next_to_archive().await?;
                    if next_to_archive < *next_needed {
                        self.add_needed_block(gossip, next_to_archive).await?;
                    }
                }
            }
            SyncMode::BlockTransfer => {
                // If there are any blocks needed that are _later_ than our currently
                // expected next block, get the next block.
                if let Some((next_needed, _)) = self.needed_blocks.first_key_value() {
                    let next_needed = *next_needed;
                    let next_chain = gossip.kolme.read().get_next_height();
                    if next_needed != next_chain {
                        self.add_needed_block(gossip, next_chain).await?;
                    }
                }
            }
            SyncMode::ValidateFrom { state_sync_height } => {
                if gossip.kolme.read().get_next_height() <= state_sync_height {
                    self.add_needed_block(gossip, state_sync_height).await?;
                }
            }
        }

        Ok(())
    }

    async fn get_data_requests_for(
        gossip: &Gossip<App>,
        height: BlockHeight,
        waiting: &mut WaitingBlock<App>,
    ) -> Result<Option<SmallVec<[DataRequest; DEFAULT_REQUEST_COUNT]>>> {
        let label = DataRequest::Block(height);
        // Check if it got added to our store in the meanwhile
        if gossip.kolme.has_block(height).await? {
            return Ok(None);
        }

        match waiting {
            WaitingBlock::Needed(status) => {
                if status.should_request(gossip, label) {
                    Ok(Some(std::iter::once(label).collect()))
                } else {
                    Ok(Some(SmallVec::new()))
                }
            }
            WaitingBlock::Received(block) => {
                // Determine if we're going to use block or state sync.
                let do_block = match gossip.sync_mode {
                    SyncMode::BlockTransfer => true,
                    SyncMode::StateTransfer | SyncMode::Archive => {
                        let kolme = gossip.kolme.read();
                        kolme.get_next_height() == block.height()
                            && kolme.get_chain_version() == kolme.get_code_version()
                    }
                    SyncMode::ValidateFrom { state_sync_height } => {
                        block.height() > state_sync_height
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
                    } else {
                        #[cfg(debug_assertions)]
                        {
                            assert!(gossip.kolme.has_block(block.height()).await.unwrap());
                        }
                    }
                    Ok(None)
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
                            pending.needed_layers.insert(hash, RequestStatus::new());
                        }
                    }

                    let res = Self::get_block_data_requests(gossip, &mut pending).await;
                    *waiting = WaitingBlock::Pending(pending);
                    res
                }
            }
            WaitingBlock::Pending(pending) => Self::get_block_data_requests(gossip, pending).await,
        }
    }

    async fn get_block_data_requests(
        gossip: &Gossip<App>,
        pending: &mut PendingBlock<App>,
    ) -> Result<Option<SmallVec<[DataRequest; DEFAULT_REQUEST_COUNT]>>> {
        let mut active_count = 0;
        let mut layers_to_drop = SmallVec::<[Sha256Hash; DEFAULT_REQUEST_COUNT]>::new();
        let mut res = SmallVec::new();
        for (hash, status) in &mut pending.needed_layers {
            if active_count >= gossip.concurrent_request_limit {
                break;
            }
            let hash = *hash;
            let label = DataRequest::Merkle(hash);
            // Check if it got added to our store in the meanwhile
            if gossip.kolme.has_merkle_hash(hash).await? {
                layers_to_drop.push(hash);
                continue;
            }

            active_count += 1;

            if status.should_request(gossip, label) {
                res.push(label);
            }
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
            gossip.kolme.resync().await?;
            Ok(None)
        } else {
            Ok(Some(res))
        }
    }

    async fn process_available_hash(
        gossip: &Gossip<App>,
        pending: &mut PendingBlock<App>,
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
                    gossip.kolme.add_merkle_layer(entry.get()).await?;

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

    pub(super) async fn add_latest_block(
        &mut self,
        gossip: &Gossip<App>,
        latest_block: &LatestBlock,
    ) {
        // All sync preferences currently want to download latest blocks.
        // So add this to the needed blocks.
        if let Err(e) = self.add_needed_block(gossip, latest_block.height).await {
            tracing::warn!(
                "{}: got error while adding needed block from a block height report: {e}",
                gossip.local_display_name
            );
        }
    }
}

pub(super) const DEFAULT_REQUEST_COUNT: usize = 8;
pub(super) const DEFAULT_WARNING_PERIOD_SECS: u64 = 5;
