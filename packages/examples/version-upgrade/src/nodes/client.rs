use anyhow::Result;
use kolme::{GossipBuilder, Kolme, KolmeStore};
use tokio::task::JoinSet;

use crate::{keys::processor_peer_id, VersionUpgradeTestApp, BOOTSTRAP_ADDRESS};

pub async fn client(version: &str) -> Result<()> {
    let kolme = Kolme::new(
        VersionUpgradeTestApp::default(),
        version,
        KolmeStore::new_in_memory(),
    )
    .await?;

    let gossip = GossipBuilder::new()
        .add_bootstrap(processor_peer_id(), BOOTSTRAP_ADDRESS.parse()?)
        .set_local_display_name(&format!("version-upgrade-client-{version}"))
        .build(kolme.clone())
        .await?;

    let mut tasks = JoinSet::new();
    tasks.spawn(gossip.run());

    while let Some(result) = tasks.join_next().await {
        result.unwrap()?;
    }

    Ok(())
}
