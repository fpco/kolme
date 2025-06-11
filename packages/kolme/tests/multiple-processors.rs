use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use anyhow::Result;
use kolme::testtasks::TestTasks;
use kolme::*;
use parking_lot::Mutex;
use rand::seq::SliceRandom;

#[derive(Clone)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
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
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(SampleState {})
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    SayHi,
}

pub fn get_sample_secret_key() -> &'static SecretKey {
    const HEX: &str = "60cb788cae86b83d8932715e99558d7b4d75b410cbbe379f232eb51fb743ca63";
    static KEY: OnceLock<SecretKey> = OnceLock::new();
    KEY.get_or_init(|| HEX.parse().unwrap())
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

impl Default for SampleKolmeApp {
    fn default() -> Self {
        let my_public_key = get_sample_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let genesis = GenesisInfo {
            kolme_ident: "Dev code".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains: ConfiguredChains::default(),
            version: DUMMY_CODE_VERSION.to_owned(),
        };

        Self { genesis }
    }
}

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
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
        return;
    }

    // Wipe out the database so we have a fresh run
    let temp = tempfile::TempDir::new().unwrap();

    let (x, y, z) = TestTasks::start(
        multiple_processors_inner,
        (block_db_str, temp.path().to_owned()),
    )
    .await;
    println!("Finished checking results of all clients, moving on to checker");
    checker(x, y, z).await.unwrap();
}

async fn multiple_processors_inner(
    test_tasks: TestTasks,
    (block_db_str, fjall_dir): (String, PathBuf),
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
        let store = KolmeStore::<SampleKolmeApp>::new_postgres(&block_db_str, &fjall_dir)
            .await
            .unwrap();
        store.clear_blocks().await.unwrap();
        None
    };

    kolme::init_logger(false, None);
    let mut kolmes = vec![];
    const PROCESSOR_COUNT: usize = 10;
    const CLIENT_COUNT: usize = 100;

    for i in 0..PROCESSOR_COUNT {
        let store = match &store {
            Some(store) => store.clone(),
            None => {
                let mut dir = fjall_dir.clone();
                dir.push(i.to_string());
                KolmeStore::new_postgres(&block_db_str, dir).await.unwrap()
            }
        };
        let kolme = Kolme::new(SampleKolmeApp::default(), DUMMY_CODE_VERSION, store)
            .await
            .unwrap();

        // TODO Ideally we would like to speed up things so this test runs much faster.
        // However, at the moment, sometimes transactions take more than
        // 10 seconds to land.
        let kolme = kolme.set_tx_await_duration(tokio::time::Duration::from_secs(60));

        let processor = Processor::new(kolme.clone(), get_sample_secret_key().clone());
        test_tasks.try_spawn_persistent(processor.run());
        test_tasks.try_spawn_persistent(check_failed_txs(kolme.clone()));
        kolmes.push(kolme);
    }

    let kolmes = Arc::<[_]>::from(kolmes);
    let all_txhashes = Arc::new(Mutex::new(HashSet::new()));
    let highest_block = Arc::new(Mutex::new(BlockHeight::start()));

    for _ in 0..CLIENT_COUNT {
        test_tasks.try_spawn(client(
            kolmes.clone(),
            all_txhashes.clone(),
            highest_block.clone(),
        ));
    }

    (kolmes, all_txhashes, highest_block)
}

async fn check_failed_txs(kolme: Kolme<SampleKolmeApp>) -> Result<()> {
    let mut recv = kolme.subscribe();
    loop {
        match recv.recv().await? {
            Notification::NewBlock(_) => (),
            Notification::GenesisInstantiation { .. } => (),
            Notification::FailedTransaction(failed) => {
                let FailedTransaction {
                    txhash,
                    error,
                    proposed_height,
                } = failed.message.as_inner();
                anyhow::bail!(
                    "Error with transaction {txhash} for block {proposed_height}: {error}"
                )
            }
            Notification::LatestBlock(_) => (),
        }
    }
}

async fn client(
    kolmes: Arc<[Kolme<SampleKolmeApp>]>,
    all_txhashes: Arc<Mutex<HashSet<TxHash>>>,
    highest_block: Arc<Mutex<BlockHeight>>,
) -> Result<()> {
    for _ in 0..10 {
        let (kolme, secret) = {
            let mut rng = rand::thread_rng();
            let kolme = (*kolmes).choose(&mut rng).unwrap();
            let secret = SecretKey::random(&mut rng);
            (kolme, secret)
        };
        let tx = Arc::new(
            kolme
                .read()
                .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi)])?,
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

    // Resynchronize all the Kolmes so they have the most up to date state from the database.
    for kolme in &*kolmes {
        kolme.resync().await.unwrap();
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
