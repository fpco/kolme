use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use anyhow::Result;
use k256::SecretKey;

use kolme::*;
use tokio::task::JoinSet;

/// In the future, move to an example and convert the binary to a library.
pub struct SampleKolmeApp;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SampleState {}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum SampleMessage {
    SayHi,
}

pub fn get_sample_secret_key() -> &'static SecretKey {
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    let mut rng = rand::thread_rng();
    KEY.get_or_init(|| SecretKey::random(&mut rng))
}

const OSMOSIS_TESTNET_CODE_ID: u64 = 123; // FIXME still need to actually write and store this contract
const NEUTRON_TESTNET_CODE_ID: u64 = 456; // FIXME still need to actually write and store this contract

const DUMMY_CODE_VERSION: &str = "dummy code version";

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let mut bridges = BTreeMap::new();
        bridges.insert(
            ExternalChain::OsmosisTestnet,
            ChainConfig {
                assets: BTreeMap::new(),
                bridge: BridgeContract::NeededCosmosBridge {
                    code_id: OSMOSIS_TESTNET_CODE_ID,
                },
            },
        );
        bridges.insert(
            ExternalChain::NeutronTestnet,
            ChainConfig {
                assets: BTreeMap::new(),
                bridge: BridgeContract::NeededCosmosBridge {
                    code_id: NEUTRON_TESTNET_CODE_ID,
                },
            },
        );
        GenesisInfo {
            kolme_ident: "Cosmos bridge example".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            executors: set,
            needed_executors: 1,
            chains: bridges,
        }
    }

    fn new_state() -> Result<Self::State> {
        Ok(SampleState {})
    }

    fn save_state(state: &Self::State) -> Result<String> {
        serde_json::to_string(state).map_err(anyhow::Error::from)
    }

    fn load_state(v: &str) -> Result<Self::State> {
        serde_json::from_str(v).map_err(anyhow::Error::from)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

async fn main_inner() -> Result<()> {
    const DB_PATH: &str = "example-cosmos-bridge.sqlite3";
    kolme::init_logger(true, None);
    let kolme = Kolme::new(SampleKolmeApp, DUMMY_CODE_VERSION, DB_PATH)
        .await
        .unwrap();

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), get_sample_secret_key().clone());
    set.spawn(processor.run_processor());

    while let Some(res) = set.join_next().await {
        match res {
            Err(e) => {
                set.abort_all();
                return Err(anyhow::anyhow!("Task panicked: {e}"));
            }
            Ok(Err(e)) => {
                set.abort_all();
                return Err(e);
            }
            Ok(Ok(())) => (),
        }
    }

    Ok(())
}
