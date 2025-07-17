use crate::kolme_app::*;
use kolme::{testtasks::TestTasks, *};

#[tokio::test]
async fn archiver() {
    kolme::init_logger(true, None);
    TestTasks::start(archiver_inner, ()).await
}

async fn archiver_inner(testtasks: TestTasks, (): ()) {
    // We're going to launch a fully working cluster, then
    // add an archiver and confirm it synchronizes with the chain.
    const IDENT: &str = "archiver";
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
        let secret = SecretKey::random();
        kolme1
            .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
    }

    let secret = SecretKey::random();
    let latest_block_height = kolme1
        .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
        .await
        .unwrap()
        .height();

    let discovery = testtasks.launch_kademlia_discovery(kolme1.clone(), "kolme1");

    // Launch the archiver, and make sure it archives.
    let kolme_archiver = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks
        .launch_kademlia_client_with(
            kolme_archiver.clone(),
            "kolme_archiver",
            &discovery,
            |gossip| gossip.set_sync_mode(SyncMode::Archive, DataLoadValidation::ValidateDataLoads),
        )
        .await;

    assert!(latest_block_height.0 > 0);

    for height in 0..=latest_block_height.0 {
        let height = BlockHeight(height);

        let block_orig = kolme1.load_block(height).await.unwrap();
        let block_archiver = tokio::time::timeout(
            tokio::time::Duration::from_millis(200),
            kolme_archiver.wait_for_block(height),
        )
        .await
        .unwrap_or_else(|_| panic!("Timed out waiting for block height {height}"))
        .unwrap();
        assert_eq!(block_orig.block.hash(), block_archiver.hash());
    }
}
