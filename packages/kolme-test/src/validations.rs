use std::sync::Arc;

use jiff::Timestamp;
use testtasks::TestTasks;

use crate::kolme_app::*;
use kolme::*;

#[tokio::test]
async fn test_invalid_hashes() {
    kolme::init_logger(true, None);
    TestTasks::start(test_invalid_hashes_inner, ()).await;
}

async fn test_invalid_hashes_inner(testtasks: TestTasks, (): ()) {
    let processor = my_secret_key();
    let kolme = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();

    testtasks.spawn_persistent(Processor::new(kolme.clone(), processor.clone()).run());

    let genesis = kolme.wait_for_block(BlockHeight(0)).await.unwrap();
    let secret = SecretKey::random();
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
        height,
    } = kolme
        .read()
        .execute_transaction(&tx, timestamp, BlockDataHandling::NoPriorData)
        .await
        .unwrap();
    assert_eq!(height, genesis.height().next());

    let block = Block {
        tx,
        timestamp: jiff::Timestamp::now(),
        processor: processor.public_key(),
        height,
        parent: genesis.hash(),
        framework_state: merkle_map::api::serialize(&framework_state).unwrap().hash(),
        app_state: merkle_map::api::serialize(&app_state).unwrap().hash(),
        logs: merkle_map::api::serialize(&logs).unwrap().hash(),
        loads,
    };

    async fn try_add(
        kolme: &Kolme<SampleKolmeApp>,
        mut block: Block<SampleMessage>,
        f: impl FnOnce(&mut Block<SampleMessage>),
    ) -> Result<(), KolmeError> {
        f(&mut block);
        let signed = TaggedJson::new(block)
            .unwrap()
            .sign(&my_secret_key())
            .unwrap();
        let signed = Arc::new(SignedBlock(signed));
        kolme.add_block(signed).await
    }

    try_add(&kolme, block.clone(), |block| {
        block.parent = BlockHash(Sha256Hash::hash(b"fake parent"))
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| block.height = BlockHeight(2))
        .await
        .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.framework_state = Sha256Hash::hash(b"fake data")
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.app_state = Sha256Hash::hash(b"fake data")
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.logs = Sha256Hash::hash(b"fake data")
    })
    .await
    .unwrap_err();

    try_add(&kolme, block.clone(), |block| {
        block.loads = vec![BlockDataLoad {
            request: "request".to_owned(),
            response: "response".to_owned(),
        }]
    })
    .await
    .unwrap_err();

    try_add(&kolme, block, |_| {}).await.unwrap();
}
