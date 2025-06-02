use anyhow::Result;
use kolme::{GossipBuilder, Kolme, KolmeStore, Processor};

use crate::keys::processor_keypair;
use crate::VersionUpgradeTestApp;
use tokio::task::JoinSet;

pub async fn processor() -> Result<()> {
    kolme::init_logger(true, None);
    let kolme = Kolme::new(
        VersionUpgradeTestApp::default(),
        "1",
        KolmeStore::new_fjall("version-upgrade-test.fjall")?,
    )
    .await?;

    let secret = kolme.clone().get_app().secret.clone();

    let processor = Processor::new(kolme.clone(), secret);
    let mut tasks = JoinSet::new();
    tasks.spawn(processor.run());

    let gossip = GossipBuilder::new()
        .add_listen_port(4546)
        .set_keypair(processor_keypair())
        .disable_mdns()
        .build(kolme)
        .await?;
    tasks.spawn(gossip.run());

    while let Some(result) = tasks.join_next().await {
        result.unwrap()?;
    }

    Ok(())
}
