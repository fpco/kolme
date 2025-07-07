use crate::*;

/// Requests all blocks from the network, ensuring we have a full history.
///
/// This should only be run together with gossip, which will perform the actual fetches.
pub struct Archiver<App: KolmeApp> {
    kolme: Kolme<App>,
}

impl<App: KolmeApp> Archiver<App> {
    pub fn new(kolme: Kolme<App>) -> Self {
        Archiver { kolme }
    }

    pub async fn run(self) -> Result<()> {
        // TODO make this more efficient by storing the known-archived-through
        // in the data store. Waiting until we simplify our data stores to make this easier to do.
        let mut next_to_archive = BlockHeight::start();
        loop {
            // OK, we can wait for the next block, which will trigger a download.
            // Once we get it, then bump the next_to_archive.
            match self.kolme.wait_for_block(next_to_archive).await {
                Ok(_) => {
                    tracing::info!("Archiver successfully waited for block {next_to_archive}");
                    next_to_archive = next_to_archive.next();
                }
                Err(e) => {
                    tracing::warn!("Archiver errored waiting for block {next_to_archive}: {e}");
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                }
            }
        }
    }
}
