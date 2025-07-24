use anyhow::Result;
use kolme::{gossip::KademliaBootstrap, *};
use testtasks::TestTasks;

#[derive(clap::Parser)]
pub(crate) struct SanityOpt {
    /// Hub to test
    hub: KademliaBootstrap,
}

pub(super) async fn sanity(opt: SanityOpt) -> Result<()> {
    init_logger(true, None);
    TestTasks::start(sanity_inner, opt).await
}

async fn sanity_inner(testtasks: TestTasks, opt: SanityOpt) -> Result<()> {
    const VERSION: &str = "v1";
    let SanityOpt { hub } = opt;

    let ident = rand::random::<u64>();
    let kolme_ident = format!("Kolme CLI sanity check: {ident}");
    let validator = SecretKey::random();
    let app = SanityApp {
        genesis: GenesisInfo {
            kolme_ident,
            validator_set: ValidatorSet {
                processor: validator.public_key(),
                listeners: std::iter::once(validator.public_key()).collect(),
                needed_listeners: 1,
                approvers: std::iter::once(validator.public_key()).collect(),
                needed_approvers: 1,
            },
            chains: ConfiguredChains::default(),
            version: VERSION.to_owned(),
        },
    };

    let kolme_processor = Kolme::new(app.clone(), VERSION, KolmeStore::new_in_memory()).await?;
    testtasks.try_spawn_persistent(Processor::new(kolme_processor.clone(), validator).run());
    testtasks.try_spawn_persistent(
        GossipBuilder::new()
            .set_local_display_name("processor")
            .add_bootstrap(hub.peer, hub.address.clone())
            .build(kolme_processor.clone())?
            .run(),
    );

    let kolme = Kolme::new(app.clone(), VERSION, KolmeStore::new_in_memory()).await?;
    testtasks.try_spawn_persistent(
        GossipBuilder::new()
            .set_local_display_name("client")
            .add_bootstrap(hub.peer, hub.address.clone())
            .build(kolme.clone())?
            .run(),
    );

    assert_eq!(kolme.read().get_app_state().count, 0);

    kolme
        .sign_propose_await_transaction(
            &SecretKey::random(),
            vec![Message::App(SanityMsg::Increment)],
        )
        .await?;

    kolme.wait_for_block(BlockHeight(1)).await?;
    assert_eq!(kolme.read().get_app_state().count, 1);

    Ok(())
}

#[derive(Clone, Debug)]
struct SanityApp {
    genesis: GenesisInfo,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
enum SanityMsg {
    Increment,
}

#[derive(Clone, Debug)]
struct SanityState {
    count: u64,
}

impl MerkleSerialize for SanityState {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { count } = self;
        serializer.store(count)?;
        Ok(())
    }
}

impl MerkleDeserialize for SanityState {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            count: deserializer.load()?,
        })
    }
}

impl KolmeApp for SanityApp {
    type State = SanityState;

    type Message = SanityMsg;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state(&self) -> Result<Self::State> {
        Ok(SanityState { count: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            SanityMsg::Increment => {
                ctx.app_state_mut().count += 1;
                Ok(())
            }
        }
    }
}
