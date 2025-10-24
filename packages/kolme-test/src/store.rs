use jiff::Timestamp;
use kolme::{
    testtasks::TestTasks, Block, BlockDataHandling, BlockHeight, ExecutionResults, Kolme,
    KolmeStore, MerkleDeserialize, MerkleMap, MerkleSerialError, MerkleSerialize, Message,
    Processor, SecretKey, SignedBlock, TaggedJson,
};
use kolme_store::sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    Executor,
};
use sqlx::PgPool;
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

    let (_pool, pg_store) = connect_pg_and_store(&postgres_url).await;

    TestTasks::start(test_block_double_insertion, pg_store).await;
}

async fn connect_pg_and_store(postgres_url: &str) -> (PgPool, KolmeStore<SampleKolmeApp>) {
    // NOTE: At startup, the postgres store hydrates the latest block height from
    // with a query, thus, we need to recreate after clear so that we truly start from scratch
    // another alternative could be just issuing the query (I think that's preferred)

    let random_u64: u64 = rand::random();
    let db_name = format!("test_db_{random_u64}");

    let maintenance_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(postgres_url)
        .await
        .expect("Failed to create maintenance pool");
    maintenance_pool
        .execute(format!(r#"CREATE DATABASE "{}""#, db_name).as_str())
        .await
        .unwrap();

    let options: PgConnectOptions = postgres_url.parse().unwrap();
    let options = options.database(&db_name);
    maintenance_pool.set_connect_options(options.clone());

    let store = KolmeStore::new_postgres_with_options(
        options,
        PgPoolOptions::new().max_connections(2),
        1024,
    )
    .await
    .unwrap();
    (maintenance_pool, store)
}

async fn test_block_double_insertion(testtasks: TestTasks, store: KolmeStore<SampleKolmeApp>) {
    // Arrange
    let processor = my_secret_key();
    let kolme = Kolme::new(SampleKolmeApp::new("Dev code"), DUMMY_CODE_VERSION, store)
        .await
        .unwrap();

    testtasks.try_spawn_persistent(Processor::new(kolme.clone(), processor.clone()).run());

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

#[derive(Debug, PartialEq, Eq)]
struct SampleState {
    pub big_map: MerkleMap<u64, u64>,
}

impl Default for SampleState {
    fn default() -> Self {
        Self {
            big_map: (0..20).map(|i| (i, i)).collect(),
        }
    }
}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), merkle_map::MerkleSerialError> {
        let Self { big_map } = self;
        serializer.store_by_hash(big_map)?;
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let big_map = deserializer.load_by_hash()?;
        Ok(Self { big_map })
    }
}

#[tokio::test]
async fn test_cache_used_when_state_exists_in_db() {
    kolme::init_logger(true, None);
    let postgres_url =
        std::env::var("PROCESSOR_BLOCK_DB").expect("Variable PROCESSOR_BLOCK_DB was missing");
    if postgres_url == "SKIP" {
        println!("Skipping test due to PROCESSOR_BLOCK_DB value of SKIP");
        return;
    }
    let (pool, _) = connect_pg_and_store(&postgres_url).await;
    sqlx::query("CREATE EXTENSION IF NOT EXISTS pg_stat_statements;")
        .execute(&pool)
        .await
        .unwrap();

    let (pool, store) = connect_pg_and_store(&postgres_url).await;

    // we save slightly larger state to get a decent number of merkle hashes
    let state = SampleState::default();

    // we use this PG store initially to save value and then create a fresh one
    let state_hash = store.save(&state).await.unwrap();

    let store = KolmeStore::<SampleKolmeApp>::new_postgres_with_options(
        pool.connect_options().as_ref().clone(),
        PgPoolOptions::new(),
        1024,
    )
    .await
    .unwrap();

    // this should be 0 as it's a fresh DB with nothing loaded yet
    let initial_count = query_merkle_contents_select_calls(&pool).await;
    tracing::info!("Initial count of calls: {initial_count}");

    tracing::info!("Loading state with our store first time");
    let loaded_state = store.load(state_hash).await.unwrap();
    assert_eq!(state, loaded_state);

    let first_load_count = query_merkle_contents_select_calls(&pool).await;
    tracing::info!("Count of calls after the first load: {first_load_count}");
    assert!(first_load_count > initial_count);

    // we save it to the store but it should be no-op as the hash is already in the DB
    store.save(&state).await.unwrap();

    let loaded_state = store.load(state_hash).await.unwrap();
    assert_eq!(state, loaded_state);

    // we expect that loading the same hash second time shouldn't touch DB as data
    // should be in merlke cache already
    let second_load_count = query_merkle_contents_select_calls(&pool).await;
    tracing::info!("Count of calls after the second load: {second_load_count}");
    assert_eq!(second_load_count, first_load_count);
}

async fn query_merkle_contents_select_calls(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT calls FROM pg_stat_statements
           WHERE query ILIKE '%FROM merkle_contents%hash=ANY($1)%' AND
           dbid = (SELECT oid FROM pg_database WHERE datname = $1)",
    )
    .bind(pool.connect_options().get_database())
    .fetch_optional(pool)
    .await
    .unwrap()
    .unwrap_or_default()
}
