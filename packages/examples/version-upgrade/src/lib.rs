use std::collections::BTreeSet;

use anyhow::Result;
use kolme::{
    ConfiguredChains, ExecutionContext, GenesisInfo, GossipBuilder, Kolme, KolmeApp, KolmeStore,
    Processor, SecretKey, ValidatorSet,
};
mod serializers;
use sha2::{Digest, Sha256};
use tokio::task::JoinSet;

#[derive(Clone)]
struct VersionUpgradeTestApp {
    secret: SecretKey,
    genesis: GenesisInfo,
}

impl VersionUpgradeTestApp {
    fn get_secret() -> SecretKey {
        // long hex string is boring, its better to use human-readable one!
        let mut hasher = Sha256::new();
        hasher.update("version upgrade test app");
        let hashed = hex::encode(hasher.finalize());
        SecretKey::from_hex(&hashed).unwrap()
    }
}

impl Default for VersionUpgradeTestApp {
    fn default() -> Self {
        let secret = Self::get_secret();
        let public_key = secret.public_key();

        let keys = BTreeSet::from([public_key]);

        let genesis = GenesisInfo {
            kolme_ident: String::from("version upgrade test genesis"),
            validator_set: ValidatorSet {
                processor: public_key,
                listeners: keys.clone(),
                needed_listeners: 1,
                approvers: keys,
                needed_approvers: 1,
            },
            chains: ConfiguredChains::default(),
        };

        Self { secret, genesis }
    }
}

#[derive(Clone, Debug)]
struct VersionUpgradeTestState {}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct VersionUpgradeTestMessage {}

impl KolmeApp for VersionUpgradeTestApp {
    type State = VersionUpgradeTestState;

    type Message = VersionUpgradeTestMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state() -> Result<Self::State> {
        Ok(Self::State {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> Result<()> {
        Ok(())
    }
}

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
        .disable_mdns()
        .build(kolme)
        .await?;
    tasks.spawn(gossip.run());

    while let Some(result) = tasks.join_next().await {
        result.unwrap()?;
    }

    Ok(())
}
