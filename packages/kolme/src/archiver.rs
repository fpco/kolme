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

    pub async fn run(self) {
        let mut next_to_archive = self
            .kolme
            .get_latest_archived_block()
            .await
            .context("Unable to retrieve latest archived height")
            .inspect_err(|err| tracing::warn!("{err:?}"))
            .ok()
            .flatten()
            .map(|height| height.next())
            .unwrap_or(BlockHeight::start());

        loop {
            // OK, we can wait for the next block, which will trigger a download.
            // Once we get it, then bump the next_to_archive.
            match self.kolme.wait_for_block(next_to_archive).await {
                Ok(_) => {
                    tracing::info!("Archiver successfully waited for block {next_to_archive}");

                    while let Err(err) = self
                        .kolme
                        .archive_block(next_to_archive)
                        .await
                        .context("Unable to store archived height")
                    {
                        tracing::warn!("Unable to save archiver block: {err:?}");
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    }

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
