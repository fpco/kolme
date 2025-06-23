use std::{
    collections::BTreeSet,
    sync::{Arc, LazyLock},
};

use anyhow::Result;

use kolme::{testtasks::TestTasks, *};
use tokio::time::{timeout, Duration};

// We only want one copy of this test running at a time
// to avoid mDNS Gossip confusion
static P2P_TEST_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SampleState {
    #[serde(default)]
    hi_count: u32,
}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.hi_count)?;
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            hi_count: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    SayHi {},
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

const DUMMY_CODE_VERSION: &str = "dummy code version";

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

impl Default for SampleKolmeApp {
    fn default() -> Self {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let genesis = GenesisInfo {
            kolme_ident: "p2p example".to_owned(),
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

    fn new_state() -> Result<Self::State> {
        Ok(SampleState { hi_count: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            SampleMessage::SayHi {} => ctx.state_mut().hi_count += 1,
        }
        Ok(())
    }
}

#[test_log::test(tokio::test)]
async fn sanity() {
    let _guard = P2P_TEST_LOCK.lock().await;
    TestTasks::start(sanity_inner, ()).await;
}

async fn sanity_inner(testtasks: TestTasks, (): ()) {
    // We're going to run two logically separate Kolmes, using different databases.
    // The first will be the processor, the second will be a client.
    // Our goal is that when we propose transactions via the client,
    // the processor picks them up, creates a new block, and that new block
    // is picked up by the client.
    let tempfile_processor = tempfile::tempdir().unwrap();
    let kolme_processor = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_fjall(tempfile_processor.path()).unwrap(),
    )
    .await
    .unwrap();
    testtasks.try_spawn_persistent(
        Processor::new(kolme_processor.clone(), my_secret_key().clone()).run(),
    );
    testtasks.try_spawn_persistent(
        GossipBuilder::new()
            .set_sync_mode(
                SyncMode::BlockTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
            .set_local_display_name("sanity-processor")
            .build(kolme_processor)
            .unwrap()
            .run(),
    );

    let tempfile_client = tempfile::tempdir().unwrap();
    let kolme_client = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_fjall(tempfile_client.path()).unwrap(),
    )
    .await
    .unwrap();
    testtasks.try_spawn_persistent(
        GossipBuilder::new()
            .set_local_display_name("sanity-client")
            .build(kolme_client.clone())
            .unwrap()
            .run(),
    );

    let secret = SecretKey::random(&mut rand::thread_rng());
    timeout(
        Duration::from_secs(30),
        kolme_client.wait_for_block(BlockHeight::start()),
    )
    .await
    .unwrap()
    .unwrap();
    let tx = Arc::new(
        kolme_client
            .read()
            .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .unwrap(),
    );
    let txhash = tx.hash();
    timeout(
        Duration::from_secs(5),
        kolme_client.propose_and_await_transaction(tx),
    )
    .await
    .unwrap()
    .unwrap();
    let block = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        kolme_client.wait_for_block(BlockHeight::start().next()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(txhash, block.0.message.as_inner().tx.hash());
}
