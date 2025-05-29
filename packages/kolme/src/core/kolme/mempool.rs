use std::{collections::VecDeque, sync::Arc};

use parking_lot::RwLock;

use crate::*;

pub struct Mempool<AppMessage> {
    txs: Arc<RwLock<Queue<AppMessage>>>,
    notify: Arc<tokio::sync::watch::Sender<usize>>,
}

#[derive(Clone)]
pub struct MempoolEntry<AppMessage> {
    pub hash: TxHash,
    pub proposed_height: Option<BlockHeight>,
    pub tx: Arc<SignedTransaction<AppMessage>>
}

type Queue<AppMessage> = VecDeque<MempoolEntry<AppMessage>>;

impl<AppMessage> Clone for Mempool<AppMessage> {
    fn clone(&self) -> Self {
        Self {
            txs: self.txs.clone(),
            notify: self.notify.clone(),
        }
    }
}

impl<AppMessage: Clone> Mempool<AppMessage> {
    pub(super) fn new() -> Self {
        Self {
            txs: Default::default(),
            notify: Arc::new(tokio::sync::watch::channel(0).0),
        }
    }

    pub(super) async fn wait_for_pool_size(&self, size: usize) {
        let mut recv = self.notify.subscribe();
        loop {
            if self.txs.read().len() == size {
                break;
            }
            recv.changed().await.unwrap();
        }
    }

    pub(super) async fn peek(&self) -> MempoolEntry<AppMessage> {
        if let Some(entry) = self.txs.read().front() {
            return entry.clone();
        }

        let mut recv = self.notify.subscribe();

        loop {
            if let Some(pair) = self.txs.read().front() {
                break pair.clone();
            }
            recv.changed().await.ok();
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
            self.notify.send_modify(|x| *x += 1);
        }
    }

    pub(super) fn add(&self, tx: Arc<SignedTransaction<AppMessage>>, proposed_height: Option<BlockHeight>) {
        self.txs.write().push_back(MempoolEntry{hash: tx.hash(), proposed_height, tx});
        self.notify.send_modify(|x| *x += 1);
    }

    pub(super) fn subscribe_additions(&self) -> tokio::sync::watch::Receiver<usize> {
        // NOTE: For now, this also notifies on removals, which is fine for our
        // use cases. If that becomes a problem in the future, we can tweak this.
        self.notify.subscribe()
    }

    pub(super) fn get_entries(&self) -> Vec<MempoolEntry<AppMessage>> {
        self.txs.read().iter().cloned().collect()
    }
}
