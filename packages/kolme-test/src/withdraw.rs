use std::{
    collections::{BTreeMap, },
    net::{SocketAddr, TcpListener},
    str::FromStr,
    time::Duration,
};

use anyhow::Result;

use kolme::{testtasks::TestTasks, *};
use tokio::time::timeout;

#[derive(Clone, Debug)]
struct WithdrawKolmeApp {
    genesis_info: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawMessage {
    Mint,
    Withdraw,
}

#[derive(Clone, Debug)]
struct WithdrawState {}

impl MerkleSerialize for WithdrawState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&1u8)?;
        Ok(())
    }
}

impl MerkleDeserialize for WithdrawState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let n = deserializer.load::<u8>()?;
        assert!(n == 1u8);
        Ok(WithdrawState {})
    }
}

impl KolmeApp for WithdrawKolmeApp {
    type State = WithdrawState;
    type Message = WithdrawMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis_info
    }

    fn new_state(&self) -> anyhow::Result<Self::State> {
        Ok(WithdrawState {})
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            WithdrawMessage::Mint => {
                ctx.mint_asset(AssetId(0), AccountId(0), kolme::Decimal::ONE_THOUSAND)?;
            }
            WithdrawMessage::Withdraw => {
                ctx.withdraw_asset(
                    AssetId(0),
                    ExternalChain::PassThrough,
                    AccountId(0),
                    &Wallet("out".into()),
                    kolme::Decimal::ONE_HUNDRED,
                )?;
            }
        }
        Ok(())
    }
}

const DUMMY_CODE_VERSION: &str = "dummy code version";

#[tokio::test]
async fn withdraw() {
    kolme::init_logger(true, None);
    TestTasks::start(withdraw_inner, ()).await
}

#[cfg(tokio_unstable)]
#[test]
fn withdraw_seed() {
    kolme::init_logger(true, None);
    let seed = tokio::runtime::RngSeed::from_bytes(b"2");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        // Pausing the clock is crucial here to ensure both tasks become ready 
        // at the *exact same logical time* after we call `tokio::time::advance`.
        // This makes the seed's role in tie-breaking very clear.
        .start_paused(true)
        .rng_seed(seed)     // Apply the seed for deterministic polling order
        .build_local(&mut Default::default())
        .unwrap();

    rt.block_on(async {
        TestTasks::start(withdraw_inner, ()).await
    });
}

async fn withdraw_inner(testtasks: TestTasks, (): ()) {
    let store1 = KolmeStore::new_in_memory();
    let port = TcpListener::bind("0.0.0.0:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();

    let processor = SecretKey::random();
    let listener = SecretKey::random();
    let approver = SecretKey::random();
    let validator_set = ValidatorSet {
        processor: processor.public_key(),
        listeners: std::iter::once(listener.public_key()).collect(),
        needed_listeners: 1,
        approvers: std::iter::once(approver.public_key()).collect(),
        needed_approvers: 1,
    };

    let kolme1 = Kolme::new(
        WithdrawKolmeApp {
            genesis_info: genesis_info(validator_set.clone(), port),
        },
        DUMMY_CODE_VERSION,
        store1.clone(),
    )
    .await
    .unwrap();

    let passthrough = kolme::pass_through::PassThrough::new();
    let addr = SocketAddr::from_str(&format!("127.0.0.1:{port}")).unwrap();
    let passthrough = tokio::task::spawn(passthrough.run(addr));
    assert!(!passthrough.is_finished());

    testtasks.try_spawn_persistent(Processor::new(kolme1.clone(), processor.clone()).run());
    let discovery = testtasks.launch_websockets_discovery(kolme1.clone(), "kolme1");
    testtasks.try_spawn_persistent(
        Listener::new(kolme1.clone(), listener.clone()).run(ChainName::PassThrough),
    );
    testtasks.try_spawn_persistent(Approver::new(kolme1.clone(), approver.clone()).run());
//    testtasks.try_spawn_persistent(Submitter::new_pass_through(kolme1.clone(), port).run());

    let store2 = KolmeStore::new_in_memory();
    let kolme2 = Kolme::new(
        WithdrawKolmeApp {
            genesis_info: genesis_info(validator_set, port),
        },
        DUMMY_CODE_VERSION,
        store2,
    )
    .await
    .unwrap();

    testtasks.launch_websockets_client_with(kolme2.clone(), "kolme2", &discovery,|builder| {
            builder.set_sync_mode(
                SyncMode::StateTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        });

    tracing::info!("Waiting start");

    timeout(
        Duration::from_secs(30),
        kolme2.wait_for_block(BlockHeight::start()),
    )
    .await
    .unwrap()
    .unwrap();

    let secret = SecretKey::random();

    tracing::info!("Minting a thousand coins");

    kolme2
        .sign_propose_await_transaction(&secret, vec![Message::App(WithdrawMessage::Mint)])
        .await
        .unwrap();

    tracing::info!("Withdrawing a hundred coins");

    kolme2
        .sign_propose_await_transaction(&secret, vec![Message::App(WithdrawMessage::Withdraw)])
        .await
        .unwrap();
}

fn genesis_info(validator_set: ValidatorSet, pass_through_port: u16) -> GenesisInfo {
    let assets = BTreeMap::from_iter([(
        AssetName("Asset One".into()),
        AssetConfig {
            decimals: 6,
            asset_id: AssetId(0),
        },
    )]);
    let mut chains = ConfiguredChains::default();
    chains
        .insert_pass_through(ChainConfig {
            assets,
            bridge: BridgeContract::Deployed(pass_through_port.to_string()),
        })
        .unwrap();

    GenesisInfo {
        kolme_ident: "SOME IDENT".into(),
        validator_set,
        chains,
        version: DUMMY_CODE_VERSION.into(),
    }
}
