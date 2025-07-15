use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;
use utils::trigger::{Trigger, TriggerSubscriber};

use crate::*;

pub struct Mempool<AppMessage> {
    txs: Arc<RwLock<Queue<AppMessage>>>,
    notify: Trigger,
    gossip_delay_duration: Duration,
}

type Queue<AppMessage> = VecDeque<MempoolItem<AppMessage>>;

struct MempoolItem<AppMessage> {
    tx: Arc<SignedTransaction<AppMessage>>,
    last_gossiped: Option<Instant>,
}

impl<AppMessage> Clone for Mempool<AppMessage> {
    fn clone(&self) -> Self {
        Self {
            txs: self.txs.clone(),
            notify: self.notify.clone(),
            gossip_delay_duration: self.gossip_delay_duration,
        }
    }
}

impl<AppMessage> Mempool<AppMessage> {
    pub(super) fn new(gossip_delay_duration: Duration) -> Self {
        Self {
            txs: Default::default(),
            notify: Trigger::new("mempool"),
            gossip_delay_duration,
        }
    }

    pub(super) async fn wait_for_pool_size(&self, size: usize) {
        let mut recv = self.notify.subscribe();
        loop {
            if self.txs.read().len() == size {
                break;
            }
            recv.listen().await;
        }
    }

    pub(super) async fn peek(&self) -> Arc<SignedTransaction<AppMessage>> {
        if let Some(item) = self.txs.read().front() {
            return item.tx.clone();
        }

        let mut recv = self.notify.subscribe();

        loop {
            if let Some(item) = self.txs.read().front() {
                break item.tx.clone();
            }
            recv.listen().await;
        }
    }

    pub(super) fn drop_tx(&self, hash: TxHash) {
        let mut modified = false;
        let mut guard = self.txs.write();
        for idx in (0..guard.len()).rev() {
            if guard[idx].tx.hash() == hash {
                guard.swap_remove_back(idx);
                modified = true;
            }
        }
        if modified {
            self.notify.trigger();
        }
    }

    pub(super) fn add(&self, tx: Arc<SignedTransaction<AppMessage>>) {
        self.txs.write().push_back(MempoolItem {
            tx,
            last_gossiped: None,
        });
        self.notify.trigger();
    }

    pub(super) fn subscribe_additions(&self) -> TriggerSubscriber {
        // NOTE: For now, this also notifies on removals, which is fine for our
        // use cases. If that becomes a problem in the future, we can tweak this.
        self.notify.subscribe()
    }

    pub(super) fn get_entries(&self) -> Vec<Arc<SignedTransaction<AppMessage>>> {
        self.txs.read().iter().map(|item| item.tx.clone()).collect()
    }

    pub(crate) fn get_entries_for_gossip(&self) -> Vec<Arc<SignedTransaction<AppMessage>>> {
        let mut cutoff = Instant::now();
        cutoff = cutoff
            .checked_sub(self.gossip_delay_duration)
            .unwrap_or(cutoff);
        self.txs
            .read()
            .iter()
            .filter_map(|item| {
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
        let mut guard = self.txs.write();
        for idx in (0..guard.len()).rev() {
            if guard[idx].tx.hash() == txhash {
                guard[idx].last_gossiped = Some(Instant::now());
            }
        }
    }
}
