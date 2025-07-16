use crate::kolme_app::*;
use kolme::{testtasks::TestTasks, *};

#[tokio::test]
async fn fast_sync() {
    kolme::init_logger(true, None);
    TestTasks::start(fast_sync_inner, ()).await
}

async fn fast_sync_inner(testtasks: TestTasks, (): ()) {
    // We're going to launch a fully working cluster, then manually
    // delete some older blocks and confirm we can fast-sync
    // just the newest block.
    const IDENT: &str = "p2p-fast";
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

    // Now delete some older blocks
    for height in BlockHeight::start().0..latest_block_height.0 {
        store1.delete_block(BlockHeight(height)).await.unwrap();
    }

    assert_eq!(latest_block_height.next(), kolme1.read().get_next_height());

    let discovery = testtasks.launch_kademlia_discovery(kolme1, "kolme1");

    // Launching a new Kolme with a new gossip set to BlockTransfer should fail
    // at syncing blocks, since the source gossip doesn't have the early blocks
    let kolme_block_transfer = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.launch_kademlia_client_with(
        kolme_block_transfer.clone(),
        "kolme_block_transfer",
        &discovery,
        |gossip| {
            gossip.set_sync_mode(
                SyncMode::BlockTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        },
    );

    // We'll check at the end of the run to confirm that this never received the latest block.
    // First check that StateTransfer works
    let kolme_state_transfer = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.launch_kademlia_client_with(
        kolme_state_transfer.clone(),
        "kolme_state_transfer",
        &discovery,
        |gossip| {
            gossip.set_sync_mode(
                SyncMode::StateTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        },
    );

    // We should be able to sync the latest block within a few seconds
    let latest_from_gossip = tokio::time::timeout(
        tokio::time::Duration::from_secs(10),
        kolme_state_transfer.wait_for_block(latest_block_height),
    )
    .await
    .expect("Timeout querying latest_block_height")
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
