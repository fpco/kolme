use std::collections::BTreeSet;

use anyhow::Result;

use kolme::{testtasks::TestTasks, *};

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SampleKolmeApp {
    pub genesis: GenesisInfo,
}

#[derive(Clone, Debug)]
pub struct SampleState {
    next_hi: u64,
    payloads: MerkleMap<u64, Vec<u8>>,
}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { next_hi, payloads } = self;
        serializer.store(next_hi)?;
        serializer.store(payloads)?;
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            next_hi: deserializer.load()?,
            payloads: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    SayHi { payload: Vec<u8> },
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

const DUMMY_CODE_VERSION: &str = "dummy code version";

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

impl SampleKolmeApp {
    fn new(ident: impl Into<String>) -> Self {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let genesis = GenesisInfo {
            kolme_ident: ident.into(),
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
        Ok(SampleState {
            next_hi: 0,
            payloads: MerkleMap::new(),
        })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            SampleMessage::SayHi { payload } => {
                let state = ctx.state_mut();
                let idx = state.next_hi;
                state.next_hi += 1;
                state.payloads.insert(idx, payload.clone());
            }
        }
        Ok(())
    }
}

#[test_log::test(tokio::test)]
async fn large_sync() {
    TestTasks::start(large_sync_inner, ()).await
}

async fn large_sync_inner(testtasks: TestTasks, (): ()) {
    // We're going to launch a fully working cluster, then manually
    // delete some older blocks and confirm we can fast-sync
    // just the newest block.
    const IDENT: &str = "p2p-large";
    let store1 = KolmeStore::new_in_memory();
    let kolme1 = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        store1.clone(),
    )
    .await
    .unwrap();

    testtasks.try_spawn_persistent(Processor::new(kolme1.clone(), my_secret_key()).run());

    // Send a few transactions to bump up the block height
    for i in 0..200 {
        let payload = std::iter::repeat(i).take(50_000).collect::<Vec<_>>();
        let secret = SecretKey::random(&mut rand::thread_rng());
        kolme1
            .sign_propose_await_transaction(
                &secret,
                vec![Message::App(SampleMessage::SayHi { payload })],
            )
            .await
            .unwrap();
    }

    let secret = SecretKey::random(&mut rand::thread_rng());
    let latest_block_height = kolme1
        .sign_propose_await_transaction(
            &secret,
            vec![Message::App(SampleMessage::SayHi {
                payload: vec![1, 2, 3],
            })],
        )
        .await
        .unwrap()
        .height();

    let latest_block = kolme1
        .get_block(latest_block_height)
        .await
        .unwrap()
        .unwrap();

    // Now delete some older blocks
    for height in BlockHeight::start().0..latest_block_height.0 {
        store1.delete_block(BlockHeight(height)).await.unwrap();
    }

    assert_eq!(latest_block_height.next(), kolme1.read().get_next_height());

    // And now launch a gossip node for this Kolme
    let discovery = testtasks.launch_kademlia_discovery(kolme1, "kolme1");

    // We'll check at the end of the run to confirm that this never received the latest block.
    // First check that StateTransfer works
    let kolme_state_transfer = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.launch_kademlia_client_with(
        kolme_state_transfer.clone(),
        "kolme_state_transfer",
        &discovery,
        |builder| {
            builder.set_sync_mode(
                SyncMode::StateTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        },
    );

    let kolme_state_transfer2 = Kolme::new(
        SampleKolmeApp::new(IDENT),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await
    .unwrap();
    testtasks.launch_kademlia_client_with(
        kolme_state_transfer2.clone(),
        "kolme_state_transfer2",
        &discovery,
        |builder| {
            builder.set_sync_mode(
                SyncMode::StateTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        },
    );

    // Due to data size, it can take a bit to transfer the entire state
    let latest_from_gossip = tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        kolme_state_transfer.wait_for_block(latest_block_height),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(latest_from_gossip.hash(), BlockHash(latest_block.blockhash));
    let latest_from_gossip2 = tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        kolme_state_transfer2.wait_for_block(latest_block_height),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        latest_from_gossip2.hash(),
        BlockHash(latest_block.blockhash)
    );
}
