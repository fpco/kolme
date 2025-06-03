use anyhow::Result;
use kolme::{GossipBuilder, Kolme, KolmeStore, Processor};

use crate::keys::{processor_keypair, processor_peer_id};
use crate::{VersionUpgradeTestApp, BOOTSTRAP_ADDRESS};
use tokio::task::JoinSet;

pub async fn processor(bootstrap: bool, version: &str) -> Result<()> {
    let kolme = Kolme::new(
        VersionUpgradeTestApp::default(),
        version,
        KolmeStore::new_fjall("version-upgrade-test.fjall")?,
    )
    .await?;

    let secret = kolme.clone().get_app().secret.clone();

    let processor = Processor::new(kolme.clone(), secret);
    let mut tasks = JoinSet::new();
    tasks.spawn(processor.run());

    let gossip_builder = GossipBuilder::new()
        .set_local_display_name(&format!("version-upgrade-processor-{version}"))
        .disable_mdns();

    let gossip_builder = if bootstrap {
        gossip_builder
            .add_listen_port(4546)
            .set_keypair(processor_keypair())
    } else {
        gossip_builder.add_bootstrap(processor_peer_id(), BOOTSTRAP_ADDRESS.parse()?)
    };

    let gossip = gossip_builder.build(kolme).await?;
    tasks.spawn(gossip.run());

    while let Some(result) = tasks.join_next().await {
        result.unwrap()?;
    }

    Ok(())
}
