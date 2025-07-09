use std::{
    collections::BTreeSet,
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::Result;
use kolme::testtasks::TestTasks;
use kolme::*;
use tokio::{sync::oneshot, time::timeout};

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

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn tx_evicted_mempool() {
    TestTasks::start(tx_evicted_inner, ()).await;
}

async fn tx_evicted_inner(test_tasks: TestTasks, (): ()) {
    let kolme = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let processor = Processor::new(kolme.clone(), get_sample_secret_key().clone());
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
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    test_tasks.launch_kademlia_client(kolme.clone(), "kolme-client", &discovery);
    test_tasks.try_spawn(client(kolme.clone(), sender));

    let kolme = Kolme::new(
        SampleKolmeApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    test_tasks.launch_kademlia_client(kolme.clone(), "kolme-no-op", &discovery);
    test_tasks.try_spawn(no_op_node(kolme.clone(), receiver));
}

async fn no_op_node(kolme: Kolme<SampleKolmeApp>, receiver: oneshot::Receiver<()>) -> Result<()> {
    let mut counter = 0;

    let mut mempool_subscribe = kolme.subscribe_mempool_additions();

    loop {
        let _ = mempool_subscribe.listen().await;
        counter += 1;
        if counter >= 10 {
            // Counter will be greater than 10 because
            // mempool_subscribe will also be triggered on removal in
            // the current implementation. But this is a good time to
            // break from the loop.
            break;
        }
    }
    receiver.await.ok();
    // Give it some time to catch up
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut attempt = 0;
    loop {
        let mempool = kolme.get_mempool_entries();
        if mempool.is_empty() {
            break;
        }
        if attempt == 10 {
            panic!(
                "Mempool is not empty after {attempt} retries. Still left {} entries.",
                mempool.len()
            );
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    Ok(())
}

async fn client(kolme: Kolme<SampleKolmeApp>, sender: oneshot::Sender<()>) -> Result<()> {
    for _ in 0..10 {
        let secret = SecretKey::random(&mut rand::thread_rng());

        let tx = Arc::new(
            kolme
                .read()
                .create_signed_transaction(&secret, vec![Message::App(SampleMessage::SayHi)])?,
        );
        let txhash = tx.hash();
        kolme.propose_and_await_transaction(tx).await.unwrap();

        let res = tokio::time::timeout(
            tokio::time::Duration::from_secs(100),
            kolme.wait_for_tx(txhash),
        )
        .await;
        match res {
            Ok(Ok(_height)) => (),
            Ok(Err(e)) => panic!("Error when checking if {txhash} is found: {e}"),
            Err(e) => panic!("txhash {txhash} not found after timeout: {e}"),
        }
    }
    sender.send(()).ok();
    Ok(())
}
