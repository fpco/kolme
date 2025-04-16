use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
};

use anyhow::{Context, Result};
use kolme::*;
use rand::Rng;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};

#[derive(Clone)]
pub struct SampleKolmeApp;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SampleState {}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(
        &self,
        _serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        _deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(SampleState {})
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum SampleMessage {
    SayHi,
}

pub fn get_sample_secret_key() -> &'static SecretKey {
    const HEX: &str = "60cb788cae86b83d8932715e99558d7b4d75b410cbbe379f232eb51fb743ca63";
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    KEY.get_or_init(|| HEX.parse().unwrap())
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: BTreeMap::new(),
        }
    }

    fn new_state() -> Result<Self::State> {
        Ok(SampleState {})
    }

    async fn execute(
        &self,
        _ctx: &mut ExecutionContext<'_, Self>,
        _msg: &Self::Message,
    ) -> Result<()> {
        // Don't need to do anything here
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 100)]
async fn multiple_processors() {
    const ENVVAR: &str = "PROCESSOR_BLOCK_DB";
    let block_db_str = match std::env::var(ENVVAR) {
        Ok(x) => x,
        Err(e) => panic!("Please set the {ENVVAR} environment variable to either SKIP or a PostgreSQL connection string: {e}")
    };
    if block_db_str == "SKIP" {
        println!("Skipping test due to no local database being available");
    }

    kolme::init_logger(false, None);
    let mut processor_set = JoinSet::new();
    let mut set = JoinSet::new();
    let mut kolmes = vec![];
    let mut readies = vec![];
    const PROCESSOR_COUNT: usize = 10;
    const CLIENT_COUNT: usize = 100;

    let tempdir = tempfile::tempdir().unwrap();

    for i in 0..PROCESSOR_COUNT {
        let mut path = tempdir.path().to_owned();
        path.push(format!("db{i}.sqlite3"));
        let kolme = Kolme::new(SampleKolmeApp, DUMMY_CODE_VERSION, path)
            .await
            .unwrap();
        let pool = sqlx::PgPool::connect(&block_db_str).await.unwrap();
        let block_db = BlockDb::new(pool).await.unwrap();
        let processor = Processor::new(
            kolme.clone(),
            get_sample_secret_key().clone(),
            Some(block_db),
        );
        readies.push(processor.ready_watcher());
        processor_set.spawn(processor.run());
        processor_set.spawn(check_failed_txs(kolme.clone()));
        kolmes.push(kolme);
    }

    for mut ready in readies {
        println!("Waiting for ready...");
        while !*ready.borrow_and_update() {
            ready.changed().await.ok();
        }
        println!("It's ready");
    }

    let kolmes = Arc::<[_]>::from(kolmes);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(CLIENT_COUNT));

    for _ in 0..CLIENT_COUNT {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        set.spawn(client(permit, kolmes.clone()));
    }

    set.spawn(checker(CLIENT_COUNT, semaphore, kolmes));

    while let Some(res) = set.join_next().await {
        res.unwrap().unwrap();
    }

    // And finally, make sure all the processors are still running
    if let Some(res) = processor_set.try_join_next() {
        panic!("A processor stopped: {res:?}");
    }
}

async fn check_failed_txs(kolme: Kolme<SampleKolmeApp>) -> Result<()> {
    let mut recv = kolme.subscribe();
    loop {
        match recv.recv().await? {
            Notification::NewBlock(_) => (),
            Notification::GenesisInstantiation { .. } => (),
            Notification::Broadcast { .. } => (),
            Notification::FailedTransaction { txhash, error } => {
                anyhow::bail!("Error with transaction {txhash}: {error}")
            }
        }
    }
}

async fn client(_: OwnedSemaphorePermit, kolmes: Arc<[Kolme<SampleKolmeApp>]>) -> Result<()> {
    for _ in 0..10 {
        let (idx, kolme, secret) = {
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..kolmes.len());
            let kolme = &kolmes[idx];
            let secret = SecretKey::random(&mut rng);
            (idx, kolme, secret)
        };
        let tx = kolme
            .read()
            .await
            .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi)])
            .await?;
        let txhash = tx.hash();
        kolme.propose_transaction(tx)?;
        println!("Proposing transaction to {idx}: {txhash}");

        // FIXME: the timeout is way too long to need to wait for this, need to investigate why
        // a shorter value of 200 fails. Looks like we have some lag in the system.
        let kolmes = kolmes.clone();
        tokio::time::timeout(tokio::time::Duration::from_millis(5000), async move {
            for kolme in &*kolmes {
                kolme.wait_for_tx(txhash).await?;
            }
            anyhow::Ok(())
        })
        .await
        .with_context(|| format!("Timed out waiting for transaction {txhash}"))??;
    }
    Ok(())
}

async fn checker(
    count: usize,
    semaphore: Arc<Semaphore>,
    kolmes: Arc<[Kolme<SampleKolmeApp>]>,
) -> Result<()> {
    let _ = semaphore.acquire_many(count as u32).await?;
    let height = kolmes[0].read().await.get_next_height();
    let hash = kolmes[0].read().await.get_current_block_hash();
    for kolme in &*kolmes {
        let kolme = kolme.read().await;
        assert_eq!(height, kolme.get_next_height());
        assert_eq!(hash, kolme.get_current_block_hash());
    }
    Ok(())
}
