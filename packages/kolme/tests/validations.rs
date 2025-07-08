use std::{
    collections::BTreeSet,
    sync::{Arc, OnceLock},
};

use jiff::Timestamp;
use testtasks::TestTasks;

use kolme::*;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
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
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(SampleState {})
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    SayHi,
}

pub fn get_sample_secret_key() -> &'static SecretKey {
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    let mut rng = rand::thread_rng();
    KEY.get_or_init(|| SecretKey::random(&mut rng))
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

impl Default for SampleKolmeApp {
    fn default() -> Self {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let genesis = GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains: ConfiguredChains::default(),
            version: DUMMY_CODE_VERSION.to_owned(),
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
        Ok(())
    }
}

#[tokio::test]
async fn test_invalid_hashes() {
    kolme::init_logger(true, None);
    TestTasks::start(test_invalid_hashes_inner, ()).await;
}

async fn test_invalid_hashes_inner(testtasks: TestTasks, (): ()) {
    let processor = get_sample_secret_key();
    let kolme = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    let mut subscription = kolme.subscribe();

    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), processor.clone()).run());
    subscription.recv().await.unwrap();

    let genesis = kolme.wait_for_block(BlockHeight(0)).await.unwrap();
    let secret = SecretKey::random(&mut rand::thread_rng());
    let tx = kolme
        .read()
        .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
        .unwrap();

    let timestamp = Timestamp::now();
    let ExecutionResults {
        framework_state,
        app_state,
        logs,
        loads,
    } = kolme
        .read()
        .execute_transaction(&tx, timestamp, BlockDataHandling::NoPriorData)
        .await
        .unwrap();

    let block = Block {
        tx,
        timestamp: jiff::Timestamp::now(),
        processor: processor.public_key(),
        height: genesis.height().next(),
        parent: genesis.hash(),
        framework_state: kolme
            .get_merkle_manager()
            .serialize(&framework_state)
            .unwrap()
            .hash,
        app_state: kolme
            .get_merkle_manager()
            .serialize(&app_state)
            .unwrap()
            .hash,
        logs: kolme.get_merkle_manager().serialize(&logs).unwrap().hash,
        loads,
    };

    async fn try_add(
        kolme: &Kolme<SampleKolmeApp>,
        mut block: Block<SampleMessage>,
        f: impl FnOnce(&mut Block<SampleMessage>),
    ) -> anyhow::Result<()> {
        f(&mut block);
        let signed = TaggedJson::new(block)
            .unwrap()
            .sign(get_sample_secret_key())
            .unwrap();
        let signed = Arc::new(SignedBlock(signed));
        kolme.add_block(signed).await
    }

    try_add(&kolme, block.clone(), |block| {
        block.parent = BlockHash(Sha256Hash::hash(b"fake parent"))
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| block.height = BlockHeight(2))
        .await
        .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.framework_state = Sha256Hash::hash(b"fake data")
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.app_state = Sha256Hash::hash(b"fake data")
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.logs = Sha256Hash::hash(b"fake data")
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.loads = vec![BlockDataLoad {
            request: "request".to_owned(),
            response: "response".to_owned(),
        }]
    })
    .await
    .unwrap_err();

    try_add(&kolme, block, |_| {}).await.unwrap();
}
