use anyhow::Result;
use kolme::*;
use testtasks::TestTasks;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SampleState {
    hi_count1: u64,
    // We could instead use a separate SampleState for the second version
    // of the app. However, I'm writing this before the PR for versioned
    // merkle serialization has landed, so avoiding that.
    hi_count2: u64,
}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self {
            hi_count1,
            hi_count2,
        } = self;
        serializer.store(hi_count1)?;
        serializer.store(hi_count2)?;
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(SampleState {
            hi_count1: deserializer.load()?,
            hi_count2: deserializer.load()?,
        })
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub enum SampleMessage {
    SayHi,
}

const VERSION1: &str = "v1";
const VERSION2: &str = "v2";

#[derive(Clone)]
pub struct SampleKolmeApp1 {
    pub genesis: GenesisInfo,
}

impl KolmeApp for SampleKolmeApp1 {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state() -> Result<Self::State> {
        Ok(SampleState {
            hi_count1: 0,
            hi_count2: 0,
        })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        SampleMessage::SayHi {}: &Self::Message,
    ) -> Result<()> {
        ctx.state_mut().hi_count1 += 1;
        Ok(())
    }
}

#[derive(Clone)]
pub struct SampleKolmeApp2 {
    pub genesis: GenesisInfo,
}

impl KolmeApp for SampleKolmeApp2 {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state() -> Result<Self::State> {
        SampleKolmeApp1::new_state()
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        SampleMessage::SayHi {}: &Self::Message,
    ) -> Result<()> {
        // This is the only difference between the two versions!
        // But it results in totally different resulting app states.
        ctx.state_mut().hi_count2 += 1;
        Ok(())
    }
}

#[tokio::test]
async fn test_upgrade() {
    kolme::init_logger(true, None);
    TestTasks::start(test_upgrade_inner, ()).await.unwrap();
}

async fn test_upgrade_inner(testtasks: TestTasks, (): ()) -> Result<()> {
    // Set up the validators and genesis info to be used for both versions of the app.
    let processor = SecretKey::random(&mut rand::thread_rng());
    let listener = SecretKey::random(&mut rand::thread_rng());
    let approver = SecretKey::random(&mut rand::thread_rng());
    let genesis = GenesisInfo {
        kolme_ident: "Dev code".to_owned(),
        validator_set: ValidatorSet {
            processor: processor.public_key(),
            listeners: std::iter::once(listener.public_key()).collect(),
            needed_listeners: 1,
            approvers: std::iter::once(approver.public_key()).collect(),
            needed_approvers: 1,
        },
        chains: ConfiguredChains::default(),
        version: VERSION1.to_owned(),
    };

    // Use Fjall for a persistent store to more closely represent a live system.
    // In practice, a shared in-memory store is likely sufficient.
    let tempdir = tempfile::TempDir::new().unwrap();

    // Launch the v1 processor...
    let store1 = KolmeStore::new_fjall(tempdir.path()).unwrap();
    let kolme1 = Kolme::new(
        SampleKolmeApp1 {
            genesis: genesis.clone(),
        },
        VERSION1,
        store1.clone(),
    )
    .await
    .unwrap();
    testtasks.try_spawn_persistent(Processor::new(kolme1.clone(), processor.clone()).run());
    let discovery = testtasks.launch_kademlia_discovery(kolme1.clone(), "kolme1");

    // And we'll launch the v2 processor immediately too, even though it won't do anything yet
    let store2 = KolmeStore::new_fjall(tempdir.path()).unwrap();
    let kolme2 = Kolme::new(SampleKolmeApp2 { genesis }, VERSION2, store2.clone())
        .await
        .unwrap();
    testtasks.try_spawn_persistent(Processor::new(kolme2.clone(), processor.clone()).run());
    testtasks
        .launch_kademlia_client_with(kolme2.clone(), "kolme2", &discovery, |builder| {
            builder.set_sync_mode(
                SyncMode::StateTransfer,
                DataLoadValidation::ValidateDataLoads,
            )
        })
        .await;

    let client = SecretKey::random(&mut rand::thread_rng());
    const HI_COUNT1: u64 = 10;
    for i in 0..HI_COUNT1 {
        kolme1
            .sign_propose_await_transaction(&client, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
        assert_eq!(
            kolme1.read().get_app_state(),
            &SampleState {
                hi_count1: i + 1,
                hi_count2: 0
            }
        );
    }

    // The chain is still on version1. So this tx will be eventually rejected by the processor.
    kolme2
        .sign_propose_await_transaction(&client, vec![Message::App(SampleMessage::SayHi {})])
        .await
        .unwrap();

    // Initiate upgrade, the first Upgrader should not be successful but the second one should.
    let next_height = kolme1.read().get_next_height();
    testtasks.spawn_persistent(Upgrader::new(kolme1.clone(), processor.clone(), VERSION2).run());
    tokio::time::timeout(
        tokio::time::Duration::from_secs(1),
        kolme1.wait_for_block(next_height),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(kolme1.read().get_chain_version(), VERSION1);

    // Now run with listener (approver would work too, doesn't matter which)
    let next_height = kolme1.read().get_next_height();
    testtasks.spawn_persistent(Upgrader::new(kolme1.clone(), listener.clone(), VERSION2).run());
    tokio::time::timeout(
        tokio::time::Duration::from_secs(1),
        kolme1.wait_for_block(next_height),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(kolme1.read().get_chain_version(), VERSION2);

    tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        kolme2.wait_for_active_version(),
    )
    .await
    .unwrap();

    const HI_COUNT2: u64 = 10;
    for i in 0..HI_COUNT2 {
        kolme2
            .sign_propose_await_transaction(&client, vec![Message::App(SampleMessage::SayHi {})])
            .await
            .unwrap();
        assert_eq!(
            kolme2.read().get_app_state(),
            &SampleState {
                hi_count1: HI_COUNT1,
                hi_count2: i + 1
            }
        );
    }

    Ok(())
}
