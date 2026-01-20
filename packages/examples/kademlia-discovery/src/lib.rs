use std::collections::BTreeSet;
use std::sync::Arc;

use absurd_future::absurd_future;
use anyhow::Result;

use kolme::*;
use tokio::task::JoinSet;
use tokio::time::{self, Duration};

const VERSION1: &str = "0.1";
const VERSION2: &str = "0.2";

#[derive(Clone, Debug)]
pub struct KademliaTestApp {
    pub genesis: GenesisInfo,
}

impl KademliaTestApp {
    fn new(listener: SecretKey, approver: SecretKey) -> KademliaTestApp {
        let my_public_key = my_secret_key().public_key();
        let mut listeners = BTreeSet::new();
        listeners.insert(listener.public_key());

        let mut approvers = BTreeSet::new();
        approvers.insert(approver.public_key());

        let genesis = GenesisInfo {
            kolme_ident: "Cosmos bridge example".to_owned(),
            validator_set: ValidatorSet {
                processor: my_public_key,
                listeners,
                needed_listeners: 1,
                approvers,
                needed_approvers: 1,
            },
            chains: ConfiguredChains::default(),
            version: VERSION1.to_owned(),
        };

        Self { genesis }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct State {
    #[serde(default)]
    hi_count: u32,
}

impl MerkleSerialize for State {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.hi_count)?;
        Ok(())
    }
}

impl MerkleDeserialize for State {
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
pub enum KademliaTestMessage {
    SayHi {},
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

// Listener
// Public key: 0339fb58c9fc020d4320058b917c26ca29ece63d1673fd564d2a5f5ec154bb86f1
// Secret key: 837b7a2747bf49b0f9b30001454cce37c331a6e4cacc610e1582aafa8fd4277b
const LISTENER_KEY_HEX: &str = "837b7a2747bf49b0f9b30001454cce37c331a6e4cacc610e1582aafa8fd4277b";
fn my_listener_key() -> SecretKey {
    SecretKey::from_hex(LISTENER_KEY_HEX).unwrap()
}

// Approver
// Public key: 027a2707f3ced7f8c729563f44a133fe605d6d794f1959232b7462d2c4aebaa577
// Secret key: 6c7d5e9c22c5bced61c060661583979e4318a990fe83dc8719e745c7fb48dbb5
const APPROVER_KEY_HEX: &str = "6c7d5e9c22c5bced61c060661583979e4318a990fe83dc8719e745c7fb48dbb5";
fn my_approver_key() -> SecretKey {
    SecretKey::from_hex(APPROVER_KEY_HEX).unwrap()
}

impl KolmeApp for KademliaTestApp {
    type State = State;
    type Message = KademliaTestMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state(&self) -> Result<Self::State, KolmeError> {
        Ok(State { hi_count: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<(), KolmeError> {
        match msg {
            KademliaTestMessage::SayHi {} => ctx.state_mut().hi_count += 1,
        }
        Ok(())
    }
}

#[derive(PartialEq, serde::Serialize, serde::Deserialize)]
struct RandomU32;

impl<App> KolmeDataRequest<App> for RandomU32 {
    type Response = u32;

    async fn load(self, _: &App) -> Result<Self::Response, KolmeDataError> {
        Ok(rand::random())
    }

    async fn validate(self, _: &App, _: &Self::Response) -> Result<(), KolmeDataError> {
        // No validation possible
        Ok(())
    }
}

pub async fn observer_node(validator_addr: &str, api_server_port: u16) -> Result<()> {
    let kolme = Kolme::new(
        KademliaTestApp::new(my_listener_key().clone(), my_approver_key().clone()),
        VERSION1,
        KolmeStore::new_in_memory(),
    )
    .await?;

    let mut set = JoinSet::new();

    let gossip = GossipBuilder::new()
        .set_local_display_name("observer")
        .set_sync_mode(
            SyncMode::BlockTransfer,
            DataLoadValidation::ValidateDataLoads,
        )
        .add_websockets_server(validator_addr)
        .build(kolme.clone())?;

    set.spawn(absurd_future(gossip.run()));

    let api = ApiServer::new(kolme);
    set.spawn(api.run(("0.0.0.0", api_server_port)));

    loop {
        tracing::info!("Continuing execution...");
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

pub async fn invalid_client(validator_addr: &str) -> Result<()> {
    let secret = SecretKey::random();

    let kolme = Kolme::new(
        KademliaTestApp::new(my_listener_key().clone(), my_approver_key().clone()),
        VERSION1,
        KolmeStore::new_in_memory(),
    )
    .await?;

    let mut set = JoinSet::new();

    let gossip = GossipBuilder::new()
        .set_duplicate_cache_time(Duration::from_secs(1))
        .add_websockets_server(validator_addr)
        .build(kolme.clone())?;

    set.spawn(gossip.run());

    let tx =
        Arc::new(kolme.read().create_signed_transaction(
            &secret,
            vec![Message::App(KademliaTestMessage::SayHi {})],
        )?);

    kolme
        .propose_and_await_transaction(tx.clone())
        .await
        .unwrap();

    tracing::info!("Going to loop");
    loop {
        // Adds tx to mempool.
        time::sleep(Duration::from_secs(5)).await;
        tracing::info!("Proposing duplicate tx: {}", tx.hash());
        kolme.propose_transaction(tx.clone()).unwrap();
    }
}

pub async fn new_version_node(api_server_port: u16) -> Result<()> {
    let kolme_store = KolmeStore::new_fjall("./fjall").unwrap();
    let kolme = Kolme::new(
        KademliaTestApp::new(my_listener_key().clone(), my_approver_key().clone()),
        VERSION2,
        kolme_store,
    )
    .await?;

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    // Processor consumes mempool transactions and add new transactions into blockchain storage.
    set.spawn(absurd_future(processor.run()));
    // Listens bridge events. Based on bridge event ID, fetches the
    // event from chain and then constructs a tx which leads to adding
    // new mempool entry.
    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run(ChainName::Cosmos));
    // Approves pending bridge actions.
    let approver = Approver::new(kolme.clone(), my_secret_key().clone());
    set.spawn(approver.run());

    let api_server = ApiServer::new(kolme.clone());
    set.spawn(api_server.run(("0.0.0.0", api_server_port)));

    let gossip = GossipBuilder::new()
        .set_duplicate_cache_time(Duration::from_secs(1))
        .add_websockets_server("ws://127.0.0.1:2006")
        .build(kolme.clone())?;
    set.spawn(absurd_future(gossip.run()));

    while let Some(res) = set.join_next().await {
        match res {
            Err(e) => {
                set.abort_all();
                return Err(anyhow::anyhow!("Task panicked: {e}"));
            }
            Ok(Err(e)) => {
                set.abort_all();
                return Err(e.into());
            }
            Ok(Ok(())) => (),
        }
    }

    Ok(())
}

pub async fn validators(
    port: u16,
    enable_api_server: bool,
    start_upgrade: bool,
    use_fjall_storage: bool,
) -> Result<()> {
    let kolme_store = if use_fjall_storage {
        KolmeStore::new_fjall("./fjall").unwrap()
    } else {
        KolmeStore::new_in_memory()
    };
    let kolme = Kolme::new(
        KademliaTestApp::new(my_listener_key().clone(), my_approver_key().clone()),
        VERSION1,
        kolme_store,
    )
    .await?;

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    // Processor consumes mempool transactions and add new transactions into blockchain storage.
    set.spawn(absurd_future(processor.run()));
    // Listens bridge events. Based on bridge event ID, fetches the
    // event from chain and then constructs a tx which leads to adding
    // new mempool entry.
    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run(ChainName::Cosmos));
    // Approves pending bridge actions.
    let approver = Approver::new(kolme.clone(), my_secret_key().clone());
    set.spawn(approver.run());
    if enable_api_server {
        let api_server = ApiServer::new(kolme.clone());
        set.spawn(api_server.run(("0.0.0.0", 2002)));
    }
    let gossip = GossipBuilder::new()
        .add_websockets_bind(format!("0.0.0.0:{port}").parse()?)
        .add_websockets_bind("0.0.0.0:2006".parse().unwrap())
        .set_duplicate_cache_time(Duration::from_secs(1))
        .build(kolme.clone())?;
    set.spawn(absurd_future(gossip.run()));

    if start_upgrade {
        let processor_upgrader = Upgrader::new(kolme.clone(), my_secret_key().clone(), VERSION2);
        let listener_upgrader = Upgrader::new(kolme.clone(), my_listener_key().clone(), VERSION2);
        let approver_upgrader = Upgrader::new(kolme, my_approver_key().clone(), VERSION2);
        set.spawn(absurd_future(processor_upgrader.run()));
        set.spawn(absurd_future(listener_upgrader.run()));
        set.spawn(absurd_future(approver_upgrader.run()));
    }

    while let Some(res) = set.join_next().await {
        match res {
            Err(e) => {
                set.abort_all();
                return Err(anyhow::anyhow!("Task panicked: {e}"));
            }
            Ok(Err(e)) => {
                set.abort_all();
                return Err(e.into());
            }
            Ok(Ok(())) => (),
        }
    }

    Ok(())
}

pub async fn client(
    validator_addr: &str,
    signing_secret: SecretKey,
    continous: bool,
) -> Result<()> {
    client_inner(validator_addr, signing_secret, continous, VERSION1).await
}

pub async fn new_node_client(
    validator_addr: &str,
    signing_secret: SecretKey,
    continous: bool,
) -> Result<()> {
    client_inner(validator_addr, signing_secret, continous, VERSION2).await
}

async fn client_inner(
    validator_addr: &str,
    signing_secret: SecretKey,
    continous: bool,
    version: &str,
) -> Result<()> {
    let kolme = Kolme::new(
        KademliaTestApp::new(my_listener_key().clone(), my_approver_key().clone()),
        version,
        KolmeStore::new_in_memory(),
    )
    .await?;

    let mut set = JoinSet::new();

    let gossip = GossipBuilder::new()
        .add_websockets_server(validator_addr)
        .build(kolme.clone())?;

    set.spawn(gossip.run());

    kolme.resync().await?;
    loop {
        let orig_next_height = kolme.read().get_next_height();
        println!("Original next height: {orig_next_height}");

        // Adds tx to mempool.
        let block = kolme
            .sign_propose_await_transaction(
                &signing_secret,
                vec![Message::App(KademliaTestMessage::SayHi {})],
            )
            .await?;
        println!("New block landed: {}", block.height());
        if !continous {
            break Ok(());
        }
        time::sleep(Duration::from_secs(10)).await;
    }
}
