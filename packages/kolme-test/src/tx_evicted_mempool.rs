use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use kolme::testtasks::TestTasks;
use kolme::*;
use tokio::{sync::oneshot, time::timeout};

use crate::kolme_app::*;

#[tokio::test(flavor = "multi_thread")]
async fn tx_evicted_mempool() {
    init_logger(true, None);
    TestTasks::start(tx_evicted_inner, ()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn evicts_same_tx_mempool() {
    TestTasks::start(evicts_same_tx_mempool_inner, ()).await;
}

async fn evicts_same_tx_mempool_inner(test_tasks: TestTasks, (): ()) {
    let kolme = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    test_tasks.try_spawn_persistent(processor.run());
    let discovery = test_tasks.launch_kademlia_discovery_with(kolme.clone(), "kolme", |g| {
        g.set_duplicate_cache_time(Duration::from_micros(100))
    });

    timeout(
        Duration::from_secs(30),
        kolme.wait_for_block(BlockHeight::start()),
    )
    .await
    .unwrap()
    .unwrap();

    let kolme: Kolme<SampleKolmeApp> = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    test_tasks.try_spawn(repeat_client(kolme.clone()));
    test_tasks
        .launch_kademlia_client_with(kolme.clone(), "kolme-client", &discovery, |item| {
            item.set_duplicate_cache_time(Duration::from_micros(100))
        })
        .await;
}

async fn repeat_client(kolme: Kolme<SampleKolmeApp>) -> Result<()> {
    let secret = SecretKey::random();

    let tx = Arc::new(
        kolme
            .read()
            .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])?,
    );
    kolme
        .propose_and_await_transaction(tx.clone())
        .await
        .unwrap();
    let mut subscription = kolme.subscribe();

    tracing::info!("Going to propose tx: {}", tx.hash());
    kolme.propose_transaction(tx);
    assert!(
        !kolme.get_mempool_entries().is_empty(),
        "Mempool should not be empty"
    );
    loop {
        match subscription.recv().await {
            Ok(note) => match note {
                Notification::NewBlock(_) => (),
                Notification::GenesisInstantiation { .. } => (),
                Notification::FailedTransaction(_) => (),
                Notification::LatestBlock(_) => (),
                Notification::EvictMempoolTransaction(_) => {
                    break;
                }
            },
            Err(_) => panic!("Error from subscription"),
        }
    }
    assert!(
        kolme.get_mempool_entries().is_empty(),
        "Mempool should be empty"
    );
    Ok(())
}

async fn tx_evicted_inner(test_tasks: TestTasks, (): ()) {
    let kolme = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    test_tasks.try_spawn_persistent(processor.run());
    let discovery = test_tasks.launch_kademlia_discovery(kolme.clone(), "kolme");

    timeout(
        Duration::from_secs(30),
        kolme.wait_for_block(BlockHeight::start()),
    )
    .await
    .unwrap()
    .unwrap();

    let kolme: Kolme<SampleKolmeApp> = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    let mutex = Arc::new(Mutex::new(Vec::new()));
    test_tasks.try_spawn(client(kolme.clone(), sender, mutex.clone()));
    test_tasks
        .launch_kademlia_client(kolme.clone(), "kolme-client", &discovery)
        .await;

    let kolme = Kolme::new(
        SampleKolmeApp::new("Dev code"),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    let kolme = kolme.set_tx_await_duration(Duration::from_secs(5));
    test_tasks.try_spawn(no_op_node(kolme.clone(), receiver, mutex));
    test_tasks
        .launch_kademlia_client(kolme, "kolme-no-op", &discovery)
        .await;
}

async fn no_op_node(
    kolme: Kolme<SampleKolmeApp>,
    receiver: oneshot::Receiver<()>,
    data: Arc<Mutex<Vec<TxHash>>>,
) -> Result<()> {
    receiver.await.ok();
    let hashes = data.lock().unwrap().clone();
    assert_eq!(hashes.len(), 5, "Five transactions expected");

    let mut attempt = 0;
    for (index, hash) in hashes.iter().enumerate() {
        loop {
            let height = kolme.wait_for_tx(*hash).await;
            if height.is_ok() {
                break;
            } else {
                attempt += 1;
            }
            if attempt >= 5 {
                let mempool = kolme.get_mempool_entries();
                panic!(
                    "Mempool is not empty after {attempt} retries for {hash} ({index}). Still left {} entries.",
                    mempool.len()
                );
            }
        }
    }
    Ok(())
}

async fn client(
    kolme: Kolme<SampleKolmeApp>,
    sender: oneshot::Sender<()>,
    data: Arc<Mutex<Vec<TxHash>>>,
) -> Result<()> {
    for _ in 0..5 {
        let secret = SecretKey::random();

        let tx = Arc::new(
            kolme
                .read()
                .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])?,
        );
        let txhash = tx.hash();
        // We propose and wait till we hear from gossip layer that a
        // new block with the same hash has come
        kolme.propose_and_await_transaction(tx).await.unwrap();
        data.lock().unwrap().push(txhash);
    }
    sender.send(()).ok();
    Ok(())
}
