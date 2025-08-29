use std::{
    num::NonZero,
    sync::Arc,
    time::{Duration, Instant},
};

use lru::LruCache;
use parking_lot::RwLock;
use utils::trigger::{Trigger, TriggerSubscriber};

use crate::*;

pub struct Mempool<AppMessage> {
    state: Arc<RwLock<MempoolState<AppMessage>>>,
    notify: Trigger,
    gossip_delay_duration: Duration,
}

struct MempoolState<AppMessage> {
    txs: LruCache<TxHash, MempoolItem<AppMessage>>,
    blocked: LruCache<TxHash, BlockReason<AppMessage>>,
}

enum BlockReason<AppMessage> {
    InBlock(Arc<SignedBlock<AppMessage>>),
    Failed(Arc<SignedTaggedJson<FailedTransaction>>),
}

impl<AppMessage> std::fmt::Debug for BlockReason<AppMessage> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BlockReason::InBlock(block) => write!(f, "already in block {}", block.height()),
            BlockReason::Failed(failed) => write!(
                f,
                "transaction already failed: {}",
                failed.message.as_inner().error
            ),
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProposeTransactionError<AppMessage> {
    #[error("Transaction is already present in mempool")]
    InMempool,
    #[error("Transaction is already present in block {}", .0.height())]
    InBlock(Arc<SignedBlock<AppMessage>>),
    #[error("Transaction failed: {}", .0.message.as_inner())]
    Failed(Arc<SignedTaggedJson<FailedTransaction>>),
}

struct MempoolItem<AppMessage> {
    tx: Arc<SignedTransaction<AppMessage>>,
    last_gossiped: Option<Instant>,
}

impl<AppMessage> Clone for Mempool<AppMessage> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            notify: self.notify.clone(),
            gossip_delay_duration: self.gossip_delay_duration,
        }
    }
}

impl<AppMessage> Mempool<AppMessage> {
    pub(super) fn new(
        gossip_delay_duration: Duration,
        tx_count: NonZero<usize>,
        blocked_tx_count: NonZero<usize>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(MempoolState {
                txs: LruCache::new(tx_count),
                blocked: LruCache::new(blocked_tx_count),
            })),
            notify: Trigger::new("mempool"),
            gossip_delay_duration,
        }
    }

    pub(super) async fn wait_for_pool_size(&self, size: usize) {
        let mut recv = self.notify.subscribe();
        loop {
            if self.state.read().txs.len() == size {
                break;
            }
            recv.listen().await;
        }
    }

    pub(super) async fn peek(&self) -> Arc<SignedTransaction<AppMessage>> {
        if let Some((_hash, item)) = self.state.read().txs.peek_lru() {
            return item.tx.clone();
        }

        let mut recv = self.notify.subscribe();

        loop {
            if let Some((_hash, item)) = self.state.read().txs.peek_lru() {
                break item.tx.clone();
            }
            recv.listen().await;
        }
    }

    /// Mark a transaction as blocked by already being in the chain.
    ///
    /// Returns [true] if this is a newly added block to the mempool.
    pub(super) fn add_signed_block(&self, block: Arc<SignedBlock<AppMessage>>) -> bool {
        let mut guard = self.state.write();
        let hash = block.tx().hash();
        if guard.blocked.contains(&hash) {
            println!("add_signed_block: already contains {hash}");
            return false;
        }
        println!("add_signed_block: adding {hash}");
        guard.blocked.put(hash, BlockReason::InBlock(block));
        guard.drop_tx(hash, &self.notify);
        self.notify.trigger();
        true
    }

    /// Mark a transaction as failed.
    ///
    /// Returns [true] if this is a newly added failed transaction.
    pub(super) fn add_failed_transaction(
        &self,
        failed: Arc<SignedTaggedJson<FailedTransaction>>,
    ) -> bool {
        let mut guard = self.state.write();
        let hash = failed.message.as_inner().txhash;
        if guard.blocked.contains(&hash) {
            return false;
        }
        guard.blocked.put(hash, BlockReason::Failed(failed));
        guard.drop_tx(hash, &self.notify);
        self.notify.trigger();
        true
    }

    pub(super) fn add(
        &self,
        tx: Arc<SignedTransaction<AppMessage>>,
    ) -> Result<(), ProposeTransactionError<AppMessage>> {
        let mut state = self.state.write();
        let hash = tx.hash();
        let res = state.blocked.get(&hash);
        println!("mempool::add: adding {hash}, got {res:?}");
        match res {
            Some(reason) => Err(match reason {
                BlockReason::InBlock(block) => ProposeTransactionError::InBlock(block.clone()),
                BlockReason::Failed(failed) => ProposeTransactionError::Failed(failed.clone()),
            }),
            None => {
                if state.txs.contains(&hash) {
                    Err(ProposeTransactionError::InMempool)
                } else {
                    state.txs.push(
                        hash,
                        MempoolItem {
                            tx,
                            last_gossiped: None,
                        },
                    );
                    self.notify.trigger();
                    Ok(())
                }
            }
        }
    }

    pub(super) fn subscribe_additions(&self) -> TriggerSubscriber {
        // NOTE: For now, this also notifies on removals, which is fine for our
        // use cases. If that becomes a problem in the future, we can tweak this.
        self.notify.subscribe()
    }

    pub(super) fn get_entries(&self) -> Vec<Arc<SignedTransaction<AppMessage>>> {
        self.state
            .read()
            .txs
            .iter()
            .map(|(_hash, item)| item.tx.clone())
            .collect()
    }

    pub(crate) fn get_entries_for_gossip(&self) -> Vec<Arc<SignedTransaction<AppMessage>>> {
        let mut cutoff = Instant::now();
        cutoff = cutoff
            .checked_sub(self.gossip_delay_duration)
            .unwrap_or(cutoff);
        self.state
            .read()
            .txs
            .iter()
            .filter_map(|(_hash, item)| {
                let to_gossip = item
                    .last_gossiped
                    .is_none_or(|last_gossiped| last_gossiped < cutoff);
                if to_gossip {
                    Some(item.tx.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub(crate) fn mark_mempool_entry_gossiped(&self, txhash: TxHash) {
        let mut guard = self.state.write();
        if let Some(item) = guard.txs.get_mut(&txhash) {
            item.last_gossiped = Some(Instant::now());
        }
    }

    pub(crate) fn remove(&self, hash: TxHash) {
        let mut guard = self.state.write();
        guard.drop_tx(hash, &self.notify);
    }
}

impl<AppMessage> MempoolState<AppMessage> {
    fn drop_tx(&mut self, hash: TxHash, notify: &Trigger) {
        if self.txs.pop(&hash).is_some() {
            notify.trigger();
        }
    }
}
