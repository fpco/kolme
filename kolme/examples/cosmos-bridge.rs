use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use anyhow::Result;
use cosmos::SeedPhrase;
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

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";
const SUBMITTER_SEED_PHRASE: &str = "blind frown harbor wet inform wing note frequent illegal garden shy across burger clay asthma kitten left august pottery napkin label already purpose best";

const OSMOSIS_TESTNET_CODE_ID: u64 = 12247;
const NEUTRON_TESTNET_CODE_ID: u64 = 11180;

const DUMMY_CODE_VERSION: &str = "dummy code version";

fn my_secret_key() -> SecretKey {
    SecretKey::from_slice(&hex::decode(SECRET_KEY_HEX).unwrap()).unwrap()
}

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = my_secret_key().public_key();
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

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    set.spawn(processor.run());
    let submitter = Submitter::new(
        kolme.clone(),
        SeedPhrase::from_str(SUBMITTER_SEED_PHRASE).unwrap(),
    );
    set.spawn(submitter.run());
    let api_server = ApiServer::new(kolme);
    set.spawn(api_server.run("[::]:3000"));

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
