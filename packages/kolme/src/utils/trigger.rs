//! Implements a basic trigger mechanism to notify a listener of work being available.
//!
//! Uses a tokio watch channel under the surface.

use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct Trigger {
    label: Arc<str>,
    sender: tokio::sync::watch::Sender<()>,
}

impl Trigger {
    pub(crate) fn new(label: impl Into<Arc<str>>) -> Self {
        Trigger {
            label: label.into(),
            sender: tokio::sync::watch::Sender::new(()),
        }
    }

    /// Create a value to subscribe to triggers.
    pub(crate) fn subscribe(&self) -> TriggerSubscriber {
        TriggerSubscriber {
            label: self.label.clone(),
            recv: self.sender.subscribe(),
        }
    }

    /// Trigger all subscribers.
    pub(crate) fn trigger(&self) {
        self.sender.send_modify(|_| ());
    }
}

/// Subscribes to a notification channel to wait for triggers.
pub struct TriggerSubscriber {
    label: Arc<str>,
    recv: tokio::sync::watch::Receiver<()>,
}

impl TriggerSubscriber {
    /// Wait for the trigger to be triggered.
    ///
    /// Note: This function will panic if the sending [Trigger] is dropped.
    pub async fn listen(&mut self) {
        if self.recv.changed().await.is_err() {
            panic!("Listening on empty trigger {}", self.label);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn sanity() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_even = counter.clone();
        let counter_odd = counter.clone();
        let trigger_even = Trigger::new("even");
        let trigger_odd = Trigger::new("odd");
        let mut recv_even = trigger_even.subscribe();
        let mut recv_odd = trigger_odd.subscribe();
        let even = tokio::task::spawn(async move {
            for i in 0..10 {
                let old = counter_even.fetch_add(1, Ordering::Relaxed);
                assert_eq!(old, i * 2);
                trigger_odd.trigger();
                recv_even.listen().await;
            }
        });
        let odd = tokio::task::spawn(async move {
            for i in 0..10 {
                recv_odd.listen().await;
                let old = counter_odd.fetch_add(1, Ordering::Relaxed);
                assert_eq!(old, i * 2 + 1);
                trigger_even.trigger();
            }
        });

        tokio::time::timeout(tokio::time::Duration::from_millis(200), even)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(tokio::time::Duration::from_millis(200), odd)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(counter.load(Ordering::Relaxed), 20);
    }
}
