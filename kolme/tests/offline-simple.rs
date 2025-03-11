use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use k256::SecretKey;

use kolme::*;

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

    fn initial_framework_state() -> RawFrameworkState {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        // FIXME add required bridges
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
        RawFrameworkState {
            assets: BTreeMap::new(),
            accounts: BTreeMap::new(),
            kolme_ident: Self::kolme_ident().into_owned(),
            code_version: DUMMY_CODE_VERSION.to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            executors: set,
            needed_executors: 1,
            chains: bridges,
        }
    }

    fn kolme_ident() -> std::borrow::Cow<'static, str> {
        "Dev code".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sample_sanity() {
        let tempfile = tempfile::NamedTempFile::new().unwrap();
        let kolme = Kolme::new(SampleKolmeApp, DUMMY_CODE_VERSION, tempfile.path())
            .await
            .unwrap();

        assert_eq!(kolme.get_next_event_height(), EventHeight::start());
        assert_eq!(kolme.get_next_state_height(), EventHeight::start());

        let processor = Processor::new(kolme.clone(), get_sample_secret_key().clone());
        processor.create_genesis_event().await.unwrap();
        assert_eq!(kolme.get_next_event_height(), EventHeight::start().next());
        assert_eq!(kolme.get_next_state_height(), EventHeight::start());
        processor.create_genesis_event().await.unwrap_err();

        processor.produce_next_state().await.unwrap();
        assert_eq!(kolme.get_next_event_height(), EventHeight::start().next());
        assert_eq!(kolme.get_next_state_height(), EventHeight::start().next());
        processor.produce_next_state().await.unwrap_err();
    }
}
