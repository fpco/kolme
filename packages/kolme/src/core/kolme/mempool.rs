use std::{collections::VecDeque, sync::Arc};

use parking_lot::RwLock;
use utils::trigger::{Trigger, TriggerSubscriber};

use crate::*;

pub struct Mempool<AppMessage> {
    txs: Arc<RwLock<Queue<AppMessage>>>,
    notify: Trigger,
}

#[derive(PartialEq)]
enum Source {
    GossipLayer,
    NonGossipLayer
}

pub struct QueueMessage<AppMessage> {
    hash: TxHash,
    message: Arc<SignedTransaction<AppMessage>>,
    source: Source,
}

type Queue<AppMessage> = VecDeque<QueueMessage<AppMessage>>;

impl<AppMessage> Clone for Mempool<AppMessage> {
    fn clone(&self) -> Self {
        Self {
            txs: self.txs.clone(),
            notify: self.notify.clone(),
        }
    }
}

impl<AppMessage> Mempool<AppMessage> {
    pub(super) fn new() -> Self {
        Self {
            txs: Default::default(),
            notify: Trigger::new("mempool"),
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

    pub(super) async fn peek(&self) -> (TxHash, Arc<SignedTransaction<AppMessage>>) {
        if let Some(pair) = self.txs.read().front() {
            return (pair.hash, pair.message.clone());
        }

        let mut recv = self.notify.subscribe();

        loop {
            if let Some(pair) = self.txs.read().front() {
                break (pair.hash, pair.message.clone());
            }
            recv.listen().await;
        }
    }

    pub(super) fn drop_tx(&self, hash: TxHash) {
        let mut guard = self.txs.write();
        let mut modified = false;
        let mut i = 0;
        while i < guard.len() {
            if guard[i].hash == hash {
                modified = true;
                guard.remove(i);
            } else {
                i += 1;
            }
        }
        if modified {
            self.notify.trigger();
        }
    }

    pub(super) fn add(&self, tx: Arc<SignedTransaction<AppMessage>>) {
        let message = QueueMessage {
            hash: tx.hash(),
            message: tx,
            source: Source::NonGossipLayer,
        };
        self.txs.write().push_back(message);
        self.notify.trigger();
    }

    pub(super) fn gossip_add(&self, tx: Arc<SignedTransaction<AppMessage>>) {
        let message = QueueMessage {
            hash: tx.hash(),
            message: tx,
            source: Source::GossipLayer,
        };
        self.txs.write().push_back(message);
        self.notify.trigger();
    }

    pub(super) fn subscribe_additions(&self) -> TriggerSubscriber {
        // NOTE: For now, this also notifies on removals, which is fine for our
        // use cases. If that becomes a problem in the future, we can tweak this.
        self.notify.subscribe()
    }

    pub(super) fn get_entries(&self) -> Vec<Arc<SignedTransaction<AppMessage>>> {
        self.txs.read().iter().map(|item| item.message.clone()).collect()
    }

    pub(super) fn get_broadcast_entries(&self) -> Vec<Arc<SignedTransaction<AppMessage>>> {
        self.txs.read().iter().filter(|item| item.source == Source::NonGossipLayer).map(|item| item.message.clone()).collect()
    }
}
