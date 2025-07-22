use jiff::Timestamp;
use kolme::{
    testtasks::TestTasks, Block, BlockDataHandling, BlockHeight, ExecutionResults, Kolme,
    KolmeStore, Message, Processor, SecretKey, SignedBlock, TaggedJson,
};
use kolme_store::sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    Executor,
};
use std::sync::Arc;

use crate::kolme_app::*;

// #[tokio::test]
// async fn test_in_memory_block_double_insertion() {
//     TestTasks::start(test_block_double_insertion, KolmeStore::new_in_memory()).await;
// }

// #[tokio::test]
// async fn test_fjall_block_double_insertion() {
//     let fjall = KolmeStore::new_fjall("logs.fjall").expect("Unable to start postgres store");
//
//     tokio::fs::remove_dir_all("logs.fjall")
//         .await
//         .expect("Unable to delete Fjall dir");
//
//     TestTasks::start(test_block_double_insertion, fjall).await;
// }

#[tokio::test]
async fn test_postgres_block_double_insertion() {
    kolme::init_logger(true, None);
    let postgres_url =
        std::env::var("PROCESSOR_BLOCK_DB").expect("Variable PROCESSOR_BLOCK_DB was missing");
    if postgres_url == "SKIP" {
        println!("Skipping test due to PROCESSOR_BLOCK_DB value of SKIP");
        return;
    }

    // NOTE: At startup, the postgres store hydrates the latest block height from
    // with a query, thus, we need to recreate after clear so that we truly start from scratch
    // another alternative could be just issuing the query (I think that's preferred)

    let random_u64: u64 = rand::random();
    let db_name = format!("test_db_{random_u64}");

    let maintenance_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&postgres_url)
        .await
        .expect("Failed to create maintenance pool");
    maintenance_pool
        .execute(format!(r#"CREATE DATABASE "{}""#, db_name).as_str())
        .await
        .unwrap();

    let options: PgConnectOptions = postgres_url.parse().unwrap();
    let options = options.database(&db_name);
    maintenance_pool.set_connect_options(options.clone());

    let postgres = KolmeStore::new_postgres_with_options(
        options,
        PgPoolOptions::new().max_connections(2),
        1024,
    )
    .await
    .unwrap();

    TestTasks::start(test_block_double_insertion, postgres).await;
}

async fn test_block_double_insertion(testtasks: TestTasks, store: KolmeStore<SampleKolmeApp>) {
    // Arrange
    let processor = my_secret_key();
    let kolme = Kolme::new(SampleKolmeApp::new("Dev code"), DUMMY_CODE_VERSION, store)
        .await
        .unwrap();

    let mut subscription = kolme.subscribe();

    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), processor.clone()).run());
    subscription.recv().await.unwrap();

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
        framework_state: merkle_map::api::serialize(&framework_state).unwrap().hash,
        app_state: merkle_map::api::serialize(&app_state).unwrap().hash,
        logs: merkle_map::api::serialize(&logs).unwrap().hash,
        loads,
    };
    fn sign_block(
        mut block: Block<SampleMessage>,
        f: impl FnOnce(&mut Block<SampleMessage>),
    ) -> anyhow::Result<Arc<SignedBlock<SampleMessage>>> {
        f(&mut block);
        let signed = TaggedJson::new(block).unwrap().sign(&my_secret_key())?;

        Ok(Arc::new(SignedBlock(signed)))
    }

    let signed_block = sign_block(block.clone(), |block| {
        block.height = BlockHeight(1);
    })
    .expect("Unable to sign first hash");
    let signed_block_duplicated = sign_block(block.clone(), |block| {
        block.height = BlockHeight(1);
    })
    .expect("Unable to sign second hash");

    // Act
    let (res1, res2) = tokio::join!(
        kolme.add_block(signed_block),
        kolme.add_block(signed_block_duplicated)
    );

    // Assert
    res1.expect("Initial insertion of Block #1 must succeed");
    res2.expect("Duplicated insertion of Block #1 must succeed");
}
