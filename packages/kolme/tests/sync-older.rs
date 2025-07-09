use std::collections::BTreeSet;

use anyhow::Result;

use kolme::{testtasks::TestTasks, *};

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

impl SampleKolmeApp {
    fn new(ident: impl Into<String>) -> Self {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let genesis = GenesisInfo {
            kolme_ident: ident.into(),
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

#[tokio::test]
async fn sync_older() {
    kolme::init_logger(true, None);
    TestTasks::start(sync_older_inner, ()).await
}

async fn sync_older_inner(testtasks: TestTasks, (): ()) {
    // Basic idea: start a new Kolme and populate some blocks.
    // Then start a second one, but give it a different code version
    // so it won't do block sync and skips the older blocks.
    //
    // Then we want to confirm three different things:
    //
    // * Using get_block fails because the block isn't found
    // * Using wait_for_block causes gossip to request the older block and succeeds
    // * Once we turn on the Archiver component, load_block works for all blocks
    const IDENT: &str = "sync-older";
    let store1 = KolmeStore::new_in_memory();
    let kolme1 = Kolme::new(
        SampleKolmeApp::new(IDENT),
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

    assert_eq!(latest_block_height.next(), kolme1.read().get_next_height());

    // And now launch a gossip node for this Kolme
    let discovery = testtasks.launch_kademlia_discovery(kolme1.clone(), "kolme1");

    // Launching a new Kolme with a different code version and gossip enabled.
    // We shouldn't get any older blocks.
    let kolme_state_transfer = Kolme::new(
        SampleKolmeApp::new(IDENT),
        "incorrect code version",
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.launch_kademlia_client_with(
        kolme_state_transfer.clone(),
        "kolme_state_transfer",
        &discovery,
        |builder| {
            builder.set_sync_mode(
                SyncMode::StateTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        },
    );

    // We should be able to sync the latest block within a few seconds
    let latest_from_gossip = tokio::time::timeout(
        tokio::time::Duration::from_secs(3),
        kolme_state_transfer.wait_for_block(latest_block_height),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(latest_from_gossip.hash(), BlockHash(latest_block.blockhash));

    // Now try an older block with get_block... it should give nothing.
    let older = BlockHeight(5);
    assert!(
        kolme_state_transfer
            .get_block(older)
            .await
            .unwrap()
            .is_none()
    );

    // But waiting should work
    let from_gossip = tokio::time::timeout(
        tokio::time::Duration::from_secs(3),
        kolme_state_transfer.wait_for_block(older),
    )
    .await
    .unwrap()
    .unwrap();
    let from_kolme1 = kolme1.load_block(older).await.unwrap();
    assert_eq!(from_gossip.hash().0, from_kolme1.blockhash);

    // OK, now launch the archive and try the same thing with every block.
    testtasks.spawn_persistent(Archiver::new(kolme_state_transfer.clone()).run());

    for height in 0..latest_block_height.0 {
        let height = BlockHeight(height);
        let from_gossip = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            kolme_state_transfer.wait_for_block(height),
        )
        .await
        .unwrap()
        .unwrap();
        let from_kolme1 = kolme1.load_block(height).await.unwrap();
        assert_eq!(from_gossip.hash().0, from_kolme1.blockhash);
    }
}

#[tokio::test]
async fn sync_older_resume() {
    kolme::init_logger(true, None);
    TestTasks::start(sync_older_resume_inner, ()).await
}

async fn sync_older_resume_inner(testtasks: TestTasks, (): ()) {
    // Start kolme, execute a 10 transactions wait for each one
    // validate that:
    //
    // - All transactions are archived
    // - When the Archiver is reestarted
    //   - Execute a new transaction
    //   - Validate it was synced
    //   - Validate that previous archived heights "updated_at" have not changed
    const IDENT: &str = "sync-older";
    let db_url = std::env::var("PROCESSOR_BLOCK_DB").expect("PROCESSOR_BLOCK_DB is missing");
    // Clear db
    let pool = sqlx::PgPool::connect(&db_url)
        .await
        .expect("Unable to connect to DB");

    sqlx::query!("TRUNCATE TABLE blocks")
        .execute(&pool)
        .await
        .expect("Unable to clear blocks table");
    sqlx::query!("TRUNCATE TABLE merkle_contents")
        .execute(&pool)
        .await
        .expect("Unable to clear merkle contents table");
    sqlx::query!("TRUNCATE TABLE archived_blocks")
        .execute(&pool)
        .await
        .expect("Unable to clear archived blocks table");
    sqlx::query!("REFRESH MATERIALIZED VIEW latest_archived_block_height")
        .execute(&pool)
        .await
        .expect("Unable to clear materialized view");

    let store = KolmeStore::new_postgres(&db_url)
        .await
        .expect("Unable to start store");
    let kolme = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        store.clone(),
    )
    .await
    .unwrap();

    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), my_secret_key()).run());
    let kolme1 = kolme.clone();
    let archiver_handle = tokio::task::spawn(Archiver::new(kolme1).run());

    for _ in 0..10 {
        let secret = SecretKey::random(&mut rand::thread_rng());
        kolme
            .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
    }

    tracing::info!("Requesting archiver to stop");
    while kolme.get_latest_archived_block().await.unwrap() != Some(BlockHeight(10)) {
        tokio::task::yield_now().await;
    }
    archiver_handle.abort();

    let initial_heights = sqlx::query!("SELECT height, archived_at FROM archived_blocks")
        .fetch_all(&pool)
        .await
        .expect("Unable to query archived blocks");

    assert_eq!(
        initial_heights.iter().map(|r| r.height).collect::<Vec<_>>(),
        (0..11).into_iter().collect::<Vec<_>>(),
        "Block heights were not archived correctly"
    );

    testtasks.spawn_persistent(Archiver::new(kolme.clone()).run());
    let secret = SecretKey::random(&mut rand::thread_rng());

    kolme
        .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
        .await
        .unwrap();

    let latest_archived_height =
        sqlx::query_scalar!(r#"SELECT height as "height!" FROM latest_archived_block_height"#)
            .fetch_one(&pool)
            .await
            .expect("Unable to retrieve latest height");

    assert_eq!(latest_archived_height, 11, "Latest height is not correct");

    let past_heights =
        sqlx::query!("SELECT height, archived_at FROM archived_blocks WHERE height <= 10")
            .fetch_all(&pool)
            .await
            .expect("Unable to query archived blocks");

    assert_eq!(
        past_heights
            .into_iter()
            .map(|record| record.archived_at)
            .collect::<Vec<_>>(),
        initial_heights
            .into_iter()
            .map(|record| record.archived_at)
            .collect::<Vec<_>>(),
        "Previous heights were resynced"
    );
}
