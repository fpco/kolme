use std::sync::Arc;

use kolme::{testtasks::TestTasks, *};
use tokio::time::{timeout, Duration};

use crate::kolme_app::*;

#[tokio::test]
async fn sanity() {
    kolme::init_logger(true, None);
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
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_fjall(tempfile_processor.path()).unwrap(),
    )
    .await
    .unwrap();
    testtasks.try_spawn_persistent(
        Processor::new(kolme_processor.clone(), my_secret_key().clone()).run(),
    );
    let discovery = testtasks.launch_kademlia_discovery(kolme_processor, "sanity-processor");

    let tempfile_client = tempfile::tempdir().unwrap();
    let kolme_client = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_fjall(tempfile_client.path()).unwrap(),
    )
    .await
    .unwrap();
    testtasks
        .launch_kademlia_client(kolme_client.clone(), "sanity-client", &discovery)
        .await;

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
