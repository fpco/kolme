use kolme::{testtasks::TestTasks, *};
use tokio::time::{timeout, Duration};

use crate::kolme_app::*;

#[tokio::test]
async fn max_tx_height() {
    init_logger(true, None);
    TestTasks::start(max_tx_height_inner, ()).await;
}

async fn max_tx_height_inner(testtasks: TestTasks, (): ()) {
    let kolme = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), my_secret_key().clone()).run());

    timeout(
        Duration::from_secs(30),
        kolme.wait_for_block(BlockHeight::start()),
    )
    .await
    .unwrap()
    .unwrap();

    for _ in 0..10 {
        let secret = SecretKey::random();
        kolme
            .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
    }
    let latest = kolme.get_latest_block().unwrap().message.as_inner().height;
    let max = BlockHeight(5);

    let secret = SecretKey::random();
    let tx_builder = TxBuilder::new()
        .add_message(Message::App(SampleMessage::SayHi {}))
        .with_max_height(max);
    let e: KolmeError = kolme
        .sign_propose_await_transaction(&secret, tx_builder)
        .await
        .unwrap_err()
        .downcast()
        .unwrap();
    match e {
        KolmeError::PastMaxHeight {
            txhash: _,
            max_height,
            proposed_height,
        } => {
            assert_eq!(latest.next(), proposed_height);
            assert_eq!(max, max_height);
        }
        _ => panic!("{e}"),
    }
}
