use crate::kolme_app::*;
use kolme::{testtasks::TestTasks, *};

#[tokio::test]
async fn migrate_stores() {
    kolme::init_logger(true, None);
    TestTasks::start(migrate_stores_inner, ()).await
}

async fn migrate_stores_inner(testtasks: TestTasks, (): ()) {
    // Run a first processor with one data store
    const IDENT: &str = "migrate-stores";
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

    // Now launch a second one with a new data store and copy over the data
    let store2 = KolmeStore::new_in_memory();
    let kolme2 = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        store2.clone(),
    )
    .await
    .unwrap();

    testtasks.try_spawn_persistent(Processor::new(kolme2.clone(), my_secret_key()).run());

    kolme2.ingest_blocks_from(&kolme1).await.unwrap();

    // Ensure the chain history is identical
    let next = kolme1.read().get_next_height();
    assert_eq!(next, kolme2.read().get_next_height());
    for height in 0..next.0 {
        let height = BlockHeight(height);
        let block1 = kolme1.get_block(height).await.unwrap().unwrap().block;
        let block2 = kolme2.get_block(height).await.unwrap().unwrap().block;
        assert_eq!(block1, block2);
    }

    // Confirm that we can still executable blocks.
    for _ in 0..10 {
        let secret = SecretKey::random(&mut rand::thread_rng());
        kolme2
            .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
    }
}
