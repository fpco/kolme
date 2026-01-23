use std::{collections::HashSet, sync::Arc};

use anyhow::Result;
use kolme::testtasks::TestTasks;
use kolme::*;
use kolme_store::sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    Executor,
};
use parking_lot::Mutex;
use rand::seq::SliceRandom;

use crate::kolme_app::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 100)]
async fn multiple_processors() {
    kolme::init_logger(true, None);
    const ENVVAR: &str = "PROCESSOR_BLOCK_DB";
    let block_db_str = match std::env::var(ENVVAR) {
        Ok(x) => x,
        Err(e) => panic!(
            "Please set the {ENVVAR} environment variable to either SKIP or a PostgreSQL connection string: {e}"
        ),
    };
    if block_db_str == "SKIP" {
        println!("Skipping test due to no local database being available");
        return;
    }

    let processor_count = match std::env::var("KOLME_PROCESSOR_COUNT") {
        Ok(x) => x.parse().unwrap_or(3usize),
        Err(_) => 3,
    };

    let client_count = match std::env::var("KOLME_CLIENT_COUNT") {
        Ok(x) => x.parse().unwrap_or(5usize),
        Err(_) => 5,
    };

    // Wipe out the database so we have a fresh run
    let (x, y, z) = TestTasks::start(
        multiple_processors_inner,
        (block_db_str, processor_count, client_count),
    )
    .await;
    println!("Finished checking results of all clients, moving on to checker");
    checker(x, y, z).await.unwrap();
}

async fn multiple_processors_inner(
    test_tasks: TestTasks,
    (block_db_str, processor_count, client_count): (String, usize, usize),
) -> (
    Arc<[Kolme<SampleKolmeApp>]>,
    Arc<Mutex<HashSet<TxHash>>>,
    Arc<Mutex<BlockHeight>>,
) {
    let store = if block_db_str == "MEMORY" {
        Some(KolmeStore::new_in_memory())
    } else if block_db_str == "FJALL" {
        Some(KolmeStore::new_fjall("fjall-dir").unwrap())
    } else {
        None
    };

    let store_pool = match store {
        Some(_) => None,
        None => {
            let random_u64: u64 = rand::random();
            let db_name = format!("test_db_{random_u64}");

            let maintenance_pool = PgPoolOptions::new()
                .max_connections(5)
                .connect(&block_db_str)
                .await
                .expect("Failed to create maintenance pool");
            maintenance_pool
                .execute(format!(r#"CREATE DATABASE "{}""#, db_name).as_str())
                .await
                .unwrap();

            let options: PgConnectOptions = block_db_str.parse().unwrap();
            let options = options.database(&db_name);
            maintenance_pool.set_connect_options(options.clone());
            Some((options, maintenance_pool))
        }
    };

    let mut kolmes = vec![];

    for _ in 0..processor_count {
        let kolme = Kolme::new(
            SampleKolmeApp::new("Dev code"),
            DUMMY_CODE_VERSION,
            match &store {
                Some(store) => store.clone(),
                None => {
                    let (options, pool) = store_pool.clone().unwrap();
                    KolmeStore::<SampleKolmeApp>::new_postgres_with_options(
                        options,
                        pool.options().clone(),
                    )
                    .await
                    .unwrap()
                }
            },
        )
        .await
        .unwrap();

        // TODO Ideally we would like to speed up things so this test runs much faster.
        // However, at the moment, sometimes transactions take more than
        // 10 seconds to land.
        let kolme = kolme.set_tx_await_duration(tokio::time::Duration::from_secs(70));

        let processor = Processor::new(kolme.clone(), my_secret_key().clone());
        test_tasks.spawn_persistent(processor.run());
        test_tasks.try_spawn_persistent(check_failed_txs(kolme.clone()));
        kolmes.push(kolme);
    }

    let kolmes = Arc::<[_]>::from(kolmes);
    let all_txhashes = Arc::new(Mutex::new(HashSet::new()));
    let highest_block = Arc::new(Mutex::new(BlockHeight::start()));

    for _ in 0..client_count {
        test_tasks.try_spawn(client(
            kolmes.clone(),
            all_txhashes.clone(),
            highest_block.clone(),
        ));
    }

    (kolmes, all_txhashes, highest_block)
}

async fn check_failed_txs(kolme: Kolme<SampleKolmeApp>) -> Result<(), KolmeError> {
    let mut failed_txs = kolme.subscribe_failed_txs();
    let failed = failed_txs.recv().await?;
    let FailedTransaction {
        txhash,
        proposed_height,
        error,
    } = failed.message.as_inner();
    Err(KolmeError::Other(format!(
        "Error with transaction {txhash} for block {proposed_height}: {error}"
    )))
}

async fn client(
    kolmes: Arc<[Kolme<SampleKolmeApp>]>,
    all_txhashes: Arc<Mutex<HashSet<TxHash>>>,
    highest_block: Arc<Mutex<BlockHeight>>,
) -> Result<(), KolmeError> {
    for _ in 0..10 {
        let (kolme, secret) = {
            let mut rng = rand::thread_rng();
            let kolme = (*kolmes).choose(&mut rng).unwrap();
            let secret = SecretKey::random();
            (kolme, secret)
        };
        let tx = Arc::new(
            kolme
                .read()
                .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])?,
        );
        let txhash = tx.hash();
        kolme.propose_and_await_transaction(tx).await?;

        {
            let mut guard = all_txhashes.lock();
            guard.insert(txhash);
            let count = guard.len();
            std::mem::drop(guard);
            if count % 50 == 0 {
                println!("In client, total transactions logged: {count}");
            }
        }

        let res = tokio::time::timeout(
            tokio::time::Duration::from_secs(100),
            kolme.wait_for_tx(txhash),
        )
        .await;
        match res {
            Ok(Ok(height)) => {
                let mut guard = highest_block.lock();
                *guard = guard.max(height);
            }
            Ok(Err(e)) => panic!("Error when checking if {txhash} is found: {e}"),
            Err(e) => panic!("txhash {txhash} not found after timeout: {e}"),
        }
    }
    Ok(())
}

async fn checker(
    kolmes: Arc<[Kolme<SampleKolmeApp>]>,
    all_txhashes: Arc<Mutex<HashSet<TxHash>>>,
    highest_block: Arc<Mutex<BlockHeight>>,
) -> Result<()> {
    let highest_block = *highest_block.lock();

    // Delay a moment to allow all the Kolmes to auto-resynchronize so they have the most up to date
    // state from the database.
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    for kolme in &*kolmes {
        assert_eq!(kolme.read().get_next_height(), highest_block.next());
    }
    let highest_block = kolmes[0]
        .read()
        .get_block(highest_block)
        .await
        .unwrap()
        .unwrap()
        .block;

    let next_height = kolmes[0].read().get_next_height();
    assert_eq!(
        next_height,
        highest_block.0.message.as_inner().height.next()
    );
    let hash = kolmes[0].read().get_current_block_hash();
    assert_eq!(hash, highest_block.hash());
    let hashes = std::mem::take(&mut *all_txhashes.lock());
    for (kolmeidx, kolme) in kolmes.iter().enumerate() {
        let kolme = kolme.read();
        assert_eq!(next_height, kolme.get_next_height());
        assert_eq!(hash, kolme.get_current_block_hash());
        for (txidx, txhash) in hashes.iter().enumerate() {
            assert!(
                kolme.get_tx_height(*txhash).await.unwrap().is_some(),
                "Transaction {txhash}#{txidx} not found in kolme#{kolmeidx}"
            );
        }
    }
    Ok(())
}
