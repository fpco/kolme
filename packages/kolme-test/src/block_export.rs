use crate::kolme_app::*;
use kolme::{testtasks::TestTasks, *};

#[tokio::test]
async fn block_export() {
    kolme::init_logger(true, None);
    TestTasks::start(block_export_inner, ()).await
}

async fn block_export_inner(testtasks: TestTasks, (): ()) {
    // We're going to launch a fully working cluster, then
    // add an archiver and confirm it synchronizes with the chain.
    const IDENT: &str = "block-export";
    let store1 = KolmeStore::new_in_memory();
    let kolme1 = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        store1.clone(),
    )
    .await
    .unwrap();

    testtasks.try_spawn_persistent(Processor::new(kolme1.clone(), my_secret_key()).run());

    for _ in 0..10 {
        let secret = SecretKey::random();
        kolme1
            .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
    }

    let tempdir = tempfile::TempDir::new().unwrap();
    let export_file = tempdir.path().join("export");

    kolme1
        .export_blocks_to(
            &export_file,
            BlockHeight::start()..kolme1.read().get_next_height(),
        )
        .await
        .unwrap();

    // Now try to load the chain data from the export.
    let store2 = KolmeStore::new_in_memory();
    let kolme2 = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        store2.clone(),
    )
    .await
    .unwrap();

    kolme2.import_blocks_from(&export_file).await.unwrap();

    assert_eq!(
        kolme1.read().get_next_height(),
        kolme2.read().get_next_height()
    );

    for height in 0..kolme2.read().get_next_height().0 {
        let height = BlockHeight(height);

        let block1 = kolme1.load_block(height).await.unwrap();
        let block2 = kolme2.load_block(height).await.unwrap();
        assert_eq!(block1.block.hash(), block2.block.hash());
    }

    testtasks.try_spawn_persistent(Processor::new(kolme2.clone(), my_secret_key()).run());

    let secret = SecretKey::random();
    kolme1
        .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
        .await
        .unwrap();
    kolme2
        .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
        .await
        .unwrap();

    assert_eq!(
        kolme1.read().get_next_height(),
        kolme2.read().get_next_height()
    );
    assert_ne!(
        kolme1.read().get_current_block_hash(),
        kolme2.read().get_current_block_hash()
    );
}
