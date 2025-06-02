use anyhow::Result;
use kolme::{GossipBuilder, Kolme, KolmeStore};
use tokio::task::JoinSet;

use crate::{keys::processor_peer_id, VersionUpgradeTestApp};

pub async fn client() -> Result<()> {
    let kolme = Kolme::new(
        VersionUpgradeTestApp::default(),
        "1",
        KolmeStore::new_in_memory(),
    )
    .await?;

    let gossip = GossipBuilder::new()
        .add_bootstrap(processor_peer_id(), "/dns4/localhost/tcp/4546".parse()?)
        .set_local_display_name("version-upgrade-client")
        .build(kolme.clone())
        .await?;

    let mut tasks = JoinSet::new();
    tasks.spawn(gossip.run());

    while let Some(result) = tasks.join_next().await {
        result.unwrap()?;
    }

    Ok(())
}
