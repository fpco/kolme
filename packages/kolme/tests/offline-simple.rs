use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use kolme::*;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SampleState {}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(
        &self,
        _serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        _deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(SampleState {})
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
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

impl Default for SampleKolmeApp {
    fn default() -> Self {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let mut bridges = ConfiguredChains::default();
        bridges
            .insert_cosmos(
                CosmosChain::OsmosisTestnet,
                ChainConfig {
                    assets: BTreeMap::new(),
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: OSMOSIS_TESTNET_CODE_ID,
                    },
                },
            )
            .unwrap();
        bridges
            .insert_cosmos(
                CosmosChain::NeutronTestnet,
                ChainConfig {
                    assets: BTreeMap::new(),
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: NEUTRON_TESTNET_CODE_ID,
                    },
                },
            )
            .unwrap();

        let genesis = GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: bridges,
        };

        Self { genesis }
    }
}

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state() -> anyhow::Result<Self::State> {
        Ok(SampleState {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("execute not implemented"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_sample_sanity(store: KolmeStore<SampleKolmeApp>) {
        let kolme = Kolme::new(SampleKolmeApp::default(), DUMMY_CODE_VERSION, store)
            .await
            .unwrap();

        assert_eq!(kolme.read().get_next_height(), BlockHeight::start());

        let processor = Processor::new(kolme.clone(), get_sample_secret_key().clone());
        processor.create_genesis_event().await.unwrap();
        assert_eq!(kolme.read().get_next_height(), BlockHeight::start().next());
        processor.create_genesis_event().await.unwrap_err();

        // processor.produce_next_state().await.unwrap();
        assert_eq!(kolme.read().get_next_height(), BlockHeight::start().next());
    }

    #[tokio::test]
    async fn test_sample_sanity_fjall() {
        let tempfile = tempfile::tempdir().unwrap();
        test_sample_sanity(KolmeStore::new_fjall(tempfile.path()).unwrap()).await
    }

    #[tokio::test]
    async fn test_sample_sanity_postgres() {
        const ENVVAR: &str = "PROCESSOR_BLOCK_DB";
        let block_db_str = match std::env::var(ENVVAR) {
            Ok(x) => x,
            Err(e) => panic!("Please set the {ENVVAR} environment variable to either SKIP or a PostgreSQL connection string: {e}")
        };
        if block_db_str == "SKIP" {
            println!("Skipping test due to no local database being available");
            return;
        }

        let tempdir = tempfile::tempdir().unwrap();
        let store = KolmeStore::new_postgres(&block_db_str, tempdir.path())
            .await
            .unwrap();
        store.clear_blocks().await.unwrap();
        test_sample_sanity(store).await
    }
}
