mod kademlia_helper;

use std::sync::{
    atomic::{AtomicBool, AtomicUsize},
    Arc,
};

use tokio::sync::mpsc::error::TryRecvError;

#[derive(Clone)]
pub struct TestTasks {
    send_keep_running: tokio::sync::watch::Sender<bool>,
    send_error: tokio::sync::mpsc::Sender<anyhow::Error>,
    running_count: Arc<AtomicUsize>,
}

impl TestTasks {
    pub async fn start<Input, Output, F, Fut>(f: F, input: Input) -> Output
    where
        F: FnOnce(TestTasks, Input) -> Fut,
        Fut: std::future::Future<Output = Output> + Send + 'static,
        Output: Send + 'static,
    {
        let (send_error, mut recv_error) = tokio::sync::mpsc::channel(16);
        let (send_keep_running, mut recv_keep_running) = tokio::sync::watch::channel(true);
        let running_count = Arc::new(AtomicUsize::new(0));
        let test_tasks = TestTasks {
            send_keep_running,
            send_error,
            running_count,
        };

        let (tx_final, mut rx_final) = tokio::sync::oneshot::channel();
        let fut = f(test_tasks.clone(), input);
        test_tasks.spawn(async move {
            let output = fut.await;
            tx_final.send(output).ok();
        });

        recv_keep_running.changed().await.unwrap();

        match recv_error.try_recv() {
            Ok(err) => {
                eprintln!("{err:?}");
                panic!("Test run failed due to panic");
            }
            Err(TryRecvError::Empty) => (),
            Err(TryRecvError::Disconnected) => unreachable!(),
        }

        rx_final.try_recv().unwrap()
    }

    pub fn spawn_persistent<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.try_spawn_persistent(async move {
            task.await;
            anyhow::Ok(())
        });
    }
    pub fn try_spawn_persistent<F, E>(&self, task: F)
    where
        F: std::future::Future<Output = Result<(), E>> + Send + 'static,
        anyhow::Error: From<E>,
    {
        self.spawn_helper(true, task)
    }

    pub fn spawn<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.try_spawn(async move {
            task.await;
            anyhow::Ok(())
        })
    }

    pub fn try_spawn<F, E>(&self, task: F)
    where
        F: std::future::Future<Output = Result<(), E>> + Send + 'static,
        anyhow::Error: From<E>,
    {
        self.spawn_helper(false, task)
    }

    fn spawn_helper<F, E>(&self, persistent: bool, task: F)
    where
        F: std::future::Future<Output = Result<(), E>> + Send + 'static,
        anyhow::Error: From<E>,
    {
        let tasks = self.clone();

        // If this is not persistent, we need to wait for it to complete before
        // exiting. Therefore, increase the running count.
        if !persistent {
            tasks
                .running_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        // Spawn the actual worker.
        let handle = tokio::spawn(async move { task.await.map_err(anyhow::Error::from) });

        // Spawn the first watchdog. It waits for the overall runtime to finish,
        // either because all tasks are done or because an error occurred,
        // and then aborts the worker. We need an AbortHandle to make this work,
        // since we'll be waiting on the handle itself in another thread.
        let was_aborted = Arc::new(AtomicBool::new(false));
        let was_aborted_clone = was_aborted.clone();
        let abort_handle = handle.abort_handle();
        let mut keep_running = tasks.send_keep_running.subscribe();
        tokio::spawn(async move {
            if *keep_running.borrow() {
                keep_running.changed().await.unwrap();
            }
            was_aborted_clone.store(true, std::sync::atomic::Ordering::Relaxed);
            abort_handle.abort();
        });

        // And spawn the second watchdog. It waits for the worker to finish and then
        // updates all book-keeping, including notifying of errors and updating the
        // running count.
        tokio::spawn(async move {
            let res = handle.await;
            let was_aborted = was_aborted.load(std::sync::atomic::Ordering::Relaxed);

            let err = if !was_aborted {
                match res {
                    Ok(Ok(())) => {
                        if persistent {
                            Some(anyhow::anyhow!("Persistent task exited unexpectedly"))
                        } else {
                            None
                        }
                    }
                    Ok(Err(e)) => Some(e.context("Task exited with an error")),
                    Err(e) => Some(anyhow::anyhow!("Task panicked: {e}")),
                }
            } else {
                None
            };

            if let Some(err) = err {
                tasks.send_error.send(err).await.ok();
                tasks.send_keep_running.send(false).ok();
            }

            if !persistent {
                let old = tasks
                    .running_count
                    .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                if old == 1 {
                    tasks.send_keep_running.send(false).ok();
                }
            }
        });
    }
}
