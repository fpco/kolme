use jiff::Timestamp;
use kolme::{
    testtasks::TestTasks, Block, BlockDataHandling, BlockHeight, ConfiguredChains,
    ExecutionContext, ExecutionResults, GenesisInfo, Kolme, KolmeApp, KolmeStore,
    MerkleDeserialize, MerkleDeserializer, MerkleSerialError, MerkleSerialize, MerkleSerializer,
    Message, Processor, SecretKey, SignedBlock, TaggedJson, ValidatorSet,
};
use std::{
    collections::BTreeSet,
    sync::{Arc, OnceLock},
};

const DUMMY_CODE_VERSION: &str = "dummy code version";

pub fn get_sample_secret_key() -> &'static SecretKey {
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    let mut rng = rand::thread_rng();
    KEY.get_or_init(|| SecretKey::random(&mut rng))
}

#[derive(Clone, Debug)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SampleState {}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum SampleMessage {
    SayHi,
}

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

#[test_log::test(tokio::test)]
async fn test_postgres_block_double_insertion() {
    let postgres_url =
        std::env::var("PROCESSOR_BLOCK_DB").expect("Variable PROCESSOR_BLOCK_DB was missing");
    let tempdir = tempfile::TempDir::new().expect("Unable to retrieve tempdir");
    let postgres = KolmeStore::new_postgres(&postgres_url, &tempdir)
        .await
        .expect("Unable to start postgres store");

    tokio::fs::remove_dir_all(&tempdir)
        .await
        .expect("Unable to delete Fjall dir");

    postgres
        .clear_blocks()
        .await
        .expect("Unable to clear postgres store");

    TestTasks::start(test_block_double_insertion, postgres).await;
}

// #[test_log::test(tokio::test)]
// async fn test_in_memory_block_double_insertion() {
//     TestTasks::start(test_block_double_insertion, KolmeStore::new_in_memory()).await;
// }

// #[test_log::test(tokio::test)]
// async fn test_fjall_block_double_insertion() {
//     let fjall = KolmeStore::new_fjall("logs.fjall").expect("Unable to start postgres store");
//
//     tokio::fs::remove_dir_all("logs.fjall")
//         .await
//         .expect("Unable to delete Fjall dir");
//
//     TestTasks::start(test_block_double_insertion, fjall).await;
// }

async fn test_block_double_insertion(testtasks: TestTasks, store: KolmeStore<SampleKolmeApp>) {
    // Arrange
    let processor = get_sample_secret_key();
    let kolme = Kolme::new(SampleKolmeApp::default(), DUMMY_CODE_VERSION, store)
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
    fn sign_block(
        mut block: Block<SampleMessage>,
        f: impl FnOnce(&mut Block<SampleMessage>),
    ) -> anyhow::Result<Arc<SignedBlock<SampleMessage>>> {
        f(&mut block);
        let signed = TaggedJson::new(block)
            .unwrap()
            .sign(get_sample_secret_key())?;

        Ok(Arc::new(SignedBlock(signed)))
    }

    let signed_block = sign_block(block.clone(), |block| {
        block.height = BlockHeight(1);
    })
    .expect("Unable to sign first hash");
    let signed_block_duplicated = sign_block(block.clone(), |block| {
        block.height = BlockHeight(1);
    })
    .expect("Unable to sign second hash");

    // Act
    let (res1, res2) = tokio::join!(
        kolme.add_block(signed_block),
        kolme.add_block(signed_block_duplicated)
    );

    // Assert
    res1.expect("Initial insertion of Block #1 must succeed");
    res2.expect("Duplicated insertion of Block #1 must succeed");
}
