use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::Result;

use gossip::GossipListener;
use kolme::*;
use libp2p::identity::Keypair;
use libp2p::PeerId;
use tokio::task::JoinSet;
use tokio::time::{self, timeout, Duration};

const DUMMY_CODE_VERSION: &str = "dummy code version";

#[derive(Clone, Debug)]
pub struct KademliaTestApp {
    pub genesis: GenesisInfo,
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

impl Default for KademliaTestApp {
    fn default() -> Self {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);

        let genesis = GenesisInfo {
            kolme_ident: "Cosmos bridge example".to_owned(),
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

impl KolmeApp for KademliaTestApp {
    type State = State;
    type Message = KademliaTestMessage;

    fn genesis_info(&self) -> &GenesisInfo {
        &self.genesis
    }

    fn new_state(&self) -> Result<Self::State> {
        Ok(State { hi_count: 0 })
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
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

    async fn load(self, _: &App) -> Result<Self::Response> {
        Ok(rand::random())
    }

    async fn validate(self, _: &App, _: &Self::Response) -> Result<()> {
        // No validation possible
        Ok(())
    }
}

pub async fn observer_node(validator_addr: &str) -> Result<()> {
    // Corresponding to the one in ../assets/validator-keypair.pem
    const VALIDATOR_PEER_ID: &str = "QmU7sxvvthsBmfVh6bg4XtodynvUhUHfWp3kWsRsnDKTew";

    const CLIENT_KEYPAIR_BYTES: &[u8] = include_bytes!("../assets/client-keypair.pk8");

    let client = Keypair::rsa_from_pkcs8(&mut CLIENT_KEYPAIR_BYTES.to_owned()).unwrap();
    let client = PeerId::from_public_key(&client.public());

    let kolme = Kolme::new(
        KademliaTestApp::default(),
        DUMMY_CODE_VERSION,
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
        .add_bootstrap(VALIDATOR_PEER_ID.parse()?, validator_addr.parse()?)
        .add_bootstrap(client, "/ip4/127.0.0.1/tcp/3001".parse()?)
        .build(kolme.clone())?;

    set.spawn(gossip.run());

    let api = ApiServer::new(kolme);
    set.spawn(api.run(("0.0.0.0", 2005)));

    loop {
        tracing::info!("Continuing execution...");
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

pub async fn invalid_client(validator_addr: &str) -> Result<()> {
    // Corresponding to the one in ../assets/validator-keypair.pem
    const VALIDATOR_PEER_ID: &str = "QmU7sxvvthsBmfVh6bg4XtodynvUhUHfWp3kWsRsnDKTew";

    let secret = SecretKey::random();

    let client_keypair: &[u8] = include_bytes!("../assets/client-keypair.pk8");

    let kolme = Kolme::new(
        KademliaTestApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await?;

    let mut set = JoinSet::new();

    let gossip = GossipBuilder::new()
        .add_listener(GossipListener {
            proto: gossip::GossipProto::Tcp,
            ip: gossip::GossipIp::Ip4,
            port: 3001,
        })
        .set_duplicate_cache_time(Duration::from_secs(1))
        .set_keypair(Keypair::rsa_from_pkcs8(&mut client_keypair.to_owned()).unwrap())
        .add_bootstrap(VALIDATOR_PEER_ID.parse()?, validator_addr.parse()?)
        .build(kolme.clone())?;

    let mut peers_connected = gossip.subscribe_network_ready();
    set.spawn(gossip.run());

    loop {
        if *peers_connected.borrow() {
            break;
        }

        timeout(Duration::from_secs(30), peers_connected.changed())
            .await
            .map_err(|_| "hit timeout while waiting for validators")
            .unwrap()
            .unwrap();
    }

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
        kolme.propose_transaction(tx.clone());
    }
}

pub async fn validators(port: u16, enable_api_server: bool) -> Result<()> {
    const VALIDATOR_KEYPAIR_BYTES: &[u8] = include_bytes!("../assets/validator-keypair.pk8");

    let kolme = Kolme::new(
        KademliaTestApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await?;

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    // Processor consumes mempool transactions and add new transactions into blockchain storage.
    set.spawn(processor.run());
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
        .add_listener(GossipListener {
            proto: gossip::GossipProto::Tcp,
            ip: gossip::GossipIp::Ip4,
            port,
        })
        .add_websockets_bind("0.0.0.0:2006".parse().unwrap())
        .set_duplicate_cache_time(Duration::from_secs(1))
        .set_keypair(Keypair::rsa_from_pkcs8(
            &mut VALIDATOR_KEYPAIR_BYTES.to_owned(),
        )?)
        .build(kolme.clone())?;
    set.spawn(gossip.run());

    while let Some(res) = set.join_next().await {
        match res {
            Err(e) => {
                set.abort_all();
                return Err(anyhow::anyhow!("Task panicked: {e}"));
            }
            Ok(Err(e)) => {
                set.abort_all();
                return Err(e);
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
    // Corresponding to the one in ../assets/validator-keypair.pem
    const VALIDATOR_PEER_ID: &str = "QmU7sxvvthsBmfVh6bg4XtodynvUhUHfWp3kWsRsnDKTew";

    let client_keypair: &[u8] = include_bytes!("../assets/client-keypair.pk8");

    let kolme = Kolme::new(
        KademliaTestApp::default(),
        DUMMY_CODE_VERSION,
        KolmeStore::new_in_memory(),
    )
    .await?;

    let mut set = JoinSet::new();

    let gossip = GossipBuilder::new()
        .add_listener(GossipListener {
            proto: gossip::GossipProto::Tcp,
            ip: gossip::GossipIp::Ip4,
            port: 3001,
        })
        .set_keypair(Keypair::rsa_from_pkcs8(&mut client_keypair.to_owned()).unwrap())
        .add_bootstrap(VALIDATOR_PEER_ID.parse()?, validator_addr.parse()?)
        .build(kolme.clone())?;

    let mut peers_connected = gossip.subscribe_network_ready();
    set.spawn(gossip.run());

    loop {
        if *peers_connected.borrow() {
            break;
        }

        timeout(Duration::from_secs(30), peers_connected.changed())
            .await
            .map_err(|_| "hit timeout while waiting for validators")
            .unwrap()
            .unwrap();
    }
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
