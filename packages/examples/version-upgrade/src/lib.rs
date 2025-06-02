use std::collections::BTreeSet;

use anyhow::Result;
use kolme::{ConfiguredChains, ExecutionContext, GenesisInfo, KolmeApp, SecretKey, ValidatorSet};
mod serializers;
use sha2::{Digest, Sha256};
pub mod nodes;

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
