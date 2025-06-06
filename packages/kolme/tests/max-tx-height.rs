use std::collections::BTreeSet;

use anyhow::Result;

use kolme::{testtasks::TestTasks, *};
use tokio::time::{timeout, Duration};

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SampleState {
    #[serde(default)]
    hi_count: u32,
}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.hi_count)?;
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            hi_count: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    SayHi {},
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

const DUMMY_CODE_VERSION: &str = "dummy code version";

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

impl Default for SampleKolmeApp {
    fn default() -> Self {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let genesis = GenesisInfo {
            kolme_ident: "max-tx-height".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners: set.clone(),
                needed_listeners: 1,
                approvers: set,
                needed_approvers: 1,
            },
            chains: ConfiguredChains::default(),
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
        Ok(SampleState { hi_count: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            SampleMessage::SayHi {} => ctx.state_mut().hi_count += 1,
        }
        Ok(())
    }
}

#[tokio::test]
async fn max_tx_height() {
    init_logger(true, None);
    TestTasks::start(max_tx_height_inner, ()).await;
}

async fn max_tx_height_inner(testtasks: TestTasks, (): ()) {
    let kolme = Kolme::new(
        SampleKolmeApp::default(),
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
        let secret = SecretKey::random(&mut rand::thread_rng());
        kolme
            .sign_propose_await_transaction(&secret, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
    }
    let latest = kolme.get_latest_block().unwrap().message.as_inner().height;
    let max = BlockHeight(5);

    let secret = SecretKey::random(&mut rand::thread_rng());
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
