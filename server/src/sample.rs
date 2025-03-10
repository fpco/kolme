use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use k256::SecretKey;

use crate::{framework_state::RawFrameworkState, prelude::*};

/// In the future, move to an example and convert the binary to a library.
pub struct SampleKolmeApp;

pub fn get_sample_secret_key() -> &'static SecretKey {
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    let mut rng = rand::thread_rng();
    KEY.get_or_init(|| SecretKey::random(&mut rng))
}

const OSMOSIS_TESTNET_CODE_ID: u64 = 123; // FIXME still need to actually write and store this contract
const NEUTRON_TESTNET_CODE_ID: u64 = 456; // FIXME still need to actually write and store this contract

impl KolmeApp for SampleKolmeApp {
    fn initial_framework_state() -> RawFrameworkState {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        // FIXME add required bridges
        let mut bridges = BTreeMap::new();
        bridges.insert(
            ExternalChain::OsmosisTestnet,
            BridgeContract::NeededCosmosBridge {
                code_id: OSMOSIS_TESTNET_CODE_ID,
            },
        );
        bridges.insert(
            ExternalChain::NeutronTestnet,
            BridgeContract::NeededCosmosBridge {
                code_id: NEUTRON_TESTNET_CODE_ID,
            },
        );
        RawFrameworkState {
            assets: BTreeMap::new(),
            accounts: BTreeMap::new(),
            kolme_ident: "initial-kolme-sample-app".to_owned(),
            code_version: Self::kolme_ident().into_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            executors: set,
            needed_executors: 1,
            bridges,
        }
    }

    fn kolme_ident() -> std::borrow::Cow<'static, str> {
        "Dev code".into()
    }
}
