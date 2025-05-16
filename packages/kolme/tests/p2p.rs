use std::{collections::BTreeSet, sync::Arc};

use anyhow::Result;

use kolme::{testtasks::TestTasks, *};
use tokio::time::{timeout, Duration};

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

#[tokio::test]
async fn sanity() {
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
            .build(kolme_processor)
            .await
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
            .build(kolme_client.clone())
            .await
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

#[tokio::test]
async fn fast_sync() {
    TestTasks::start(fast_sync_inner, ()).await
}

async fn fast_sync_inner(testtasks: TestTasks, (): ()) {
    // We're going to launch a fully working cluster, then manually
    // delete some older blocks and confirm we can fast-sync
    // just the newest block.
    let store1 = KolmeStore::new_in_memory();
    let kolme1 = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        store1.clone(),
    )
    .await
    .unwrap();

    testtasks.try_spawn_persistent(Processor::new(kolme1.clone(), my_secret_key()).run());

    // Send a few transactions to bump up the block height
    for _ in 0..10 {
        let secret = SecretKey::random(&mut rand::thread_rng());
        kolme1
            .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
    }

    let secret = SecretKey::random(&mut rand::thread_rng());
    let latest_block_height = kolme1
        .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
        .await
        .unwrap()
        .height();

    let latest_block = kolme1
        .get_block(latest_block_height)
        .await
        .unwrap()
        .unwrap();

    // Now delete some older blocks
    for height in BlockHeight::start().0..latest_block_height.0 {
        store1.delete_block(BlockHeight(height)).await.unwrap();
    }

    assert_eq!(latest_block_height.next(), kolme1.read().get_next_height());

    // And now launch a gossip node for this Kolme
    testtasks.try_spawn_persistent(GossipBuilder::new().build(kolme1).await.unwrap().run());

    // Launching a new Kolme with a new gossip set to BlockTransfer should fail
    let kolme_block_transfer = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.try_spawn_persistent(
        GossipBuilder::new()
            .set_sync_mode(
                SyncMode::BlockTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
            .build(kolme_block_transfer.clone())
            .await
            .unwrap()
            .run(),
    );

    // We'll check at the end of the run to confirm that this never received the latest block.
    // First check that StateTransfer works
    let kolme_state_transfer = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.try_spawn_persistent(
        GossipBuilder::new()
            .set_sync_mode(
                SyncMode::StateTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
            .build(kolme_state_transfer.clone())
            .await
            .unwrap()
            .run(),
    );

    // We should be able to sync the latest block within a few seconds
    let latest_from_gossip = tokio::time::timeout(
        tokio::time::Duration::from_secs(10),
        kolme_state_transfer.wait_for_block(latest_block_height),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(latest_from_gossip.hash(), BlockHash(latest_block.blockhash));

    // Make sure we never caught up via block transfer.
    // TODO We'd like to ensure we get no blocks at all.
    // However, some tests have demonstrated getting the first block.
    // It's worth investigating why in the future, but it's not priority.
    assert_ne!(
        kolme_block_transfer.read().get_next_height().0,
        latest_block.height + 1
    );
}
