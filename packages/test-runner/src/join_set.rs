use std::thread::Thread;

use anyhow::Result;

pub struct JoinSet {
    parent: Thread,
    handles: Vec<std::thread::JoinHandle<Result<()>>>,
}

pub fn try_join<F: FnOnce(&mut JoinSet)>(f: F) -> Result<()> {
    let mut join_set = JoinSet {
        parent: std::thread::current(),
        handles: vec![],
    };
    f(&mut join_set);

    while !join_set.handles.is_empty() {
        // Could probably just use park, I'm being overly cautious here.
        std::thread::park_timeout(std::time::Duration::from_secs(5));

        let mut i = 0;
        while i < join_set.handles.len() {
            if join_set.handles[i].is_finished() {
                let handle = join_set.handles.swap_remove(i);
                handle
                    .join()
                    .map_err(|e| anyhow::anyhow!("JoinSet: task panicked: {e:?}"))??;
            } else {
                i += 1;
            }
        }
    }

    Ok(())
}

impl JoinSet {
    pub fn spawn<F: FnOnce() -> Result<()> + Send + 'static>(&mut self, f: F) {
        let dropper = UnparkOnDrop(self.parent.clone());
        let handle = std::thread::spawn(move || {
            let _dropper = dropper;
            f()
        });
        self.handles.push(handle);
    }
}

struct UnparkOnDrop(Thread);

impl Drop for UnparkOnDrop {
    fn drop(&mut self) {
        self.0.unpark();
    }
}
