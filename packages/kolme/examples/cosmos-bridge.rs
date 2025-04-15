use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    str::FromStr,
};

use anyhow::Result;
use clap::Parser;
use cosmos::{HasAddressHrp, SeedPhrase};

use kolme::*;
use tokio::task::JoinSet;

/// In the future, move to an example and convert the binary to a library.
#[derive(Clone, Debug)]
pub struct SampleKolmeApp;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SampleState {
    #[serde(default)]
    hi_count: u32,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SampleMessage {
    SayHi {},
    /// Generate a random number, for no particular reason at all
    Random {},
    /// The address itself tells us whether to send to Osmosis or Neutron
    /// FIXME: maybe this should be part of the bank module instead. Then our bridge is just a default Kolme app!
    SendTo {
        address: cosmos::Address,
        amount: Decimal,
    },
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";
const SUBMITTER_SEED_PHRASE: &str = "blind frown harbor wet inform wing note frequent illegal garden shy across burger clay asthma kitten left august pottery napkin label already purpose best";

const OSMOSIS_TESTNET_CODE_ID: u64 = 12279;
const NEUTRON_TESTNET_CODE_ID: u64 = 11328;

const DUMMY_CODE_VERSION: &str = "dummy code version";

fn my_secret_key() -> SecretKey {
    SecretKey::from_hex(SECRET_KEY_HEX).unwrap()
}

impl KolmeApp for SampleKolmeApp {
    type State = SampleState;
    type Message = SampleMessage;

    fn genesis_info() -> GenesisInfo {
        let my_public_key = my_secret_key().public_key();
        let mut set = BTreeSet::new();
        set.insert(my_public_key);
        let mut bridges = ConfiguredChains::default();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName(
                "factory/osmo1mgcky4e24969532hee55ly4rrl30z4tkzgfvq7/kolmeoutgoing".to_owned(),
            ),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );
        bridges
            .insert_cosmos(
                CosmosChain::OsmosisTestnet,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: OSMOSIS_TESTNET_CODE_ID,
                    },
                },
            )
            .unwrap();
        let mut assets = BTreeMap::new();
        assets.insert(
            AssetName(
                "factory/neutron1mgcky4e24969532hee55ly4rrl30z4tkwvn7vt/kolmeincoming".to_owned(),
            ),
            AssetConfig {
                decimals: 6,
                asset_id: AssetId(1),
            },
        );
        bridges
            .insert_cosmos(
                CosmosChain::NeutronTestnet,
                ChainConfig {
                    assets,
                    bridge: BridgeContract::NeededCosmosBridge {
                        code_id: NEUTRON_TESTNET_CODE_ID,
                    },
                },
            )
            .unwrap();
        GenesisInfo {
            kolme_ident: "Cosmos bridge example".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: bridges,
        }
    }

    fn new_state() -> Result<Self::State> {
        Ok(SampleState { hi_count: 0 })
    }

    fn save_state(state: &Self::State) -> Result<String> {
        serde_json::to_string(state).map_err(anyhow::Error::from)
    }

    fn load_state(v: &str) -> Result<Self::State> {
        serde_json::from_str(v).map_err(anyhow::Error::from)
    }

    async fn execute(
        &self,
        ctx: &mut ExecutionContext<'_, Self>,
        msg: &Self::Message,
    ) -> Result<()> {
        match msg {
            SampleMessage::SayHi {} => ctx.state_mut().hi_count += 1,
            SampleMessage::SendTo { address, amount } => {
                let chain = match address.get_address_hrp().as_str() {
                    "osmo" => ExternalChain::OsmosisTestnet,
                    "neutron" => ExternalChain::NeutronTestnet,
                    _ => anyhow::bail!("Unsupported wallet address: {address}"),
                };
                ctx.withdraw_asset(
                    AssetId(1),
                    chain,
                    ctx.get_sender_id(),
                    &Wallet(address.to_string()),
                    *amount,
                )?;
            }
            SampleMessage::Random {} => {
                let value = ctx.load_data(RandomU32).await?;
                ctx.log(format!("Calculated a random number: {value}"));
            }
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

#[derive(clap::Parser)]
struct Opt {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Parser)]
enum Cmd {
    Serve {
        #[clap(long, default_value = "[::]:3000")]
        bind: SocketAddr,
    },
    /// Generate a new public/secret keypair
    GenPair {},
    /// Sign an arbitrary message with the given secret key
    Sign {
        #[clap(long)]
        payload: String,
        /// Hex-encoded secret key
        #[clap(long)]
        secret: String,
    },
    Broadcast {
        #[clap(long, default_value = r#"{"say_hi":{}}"#)]
        message: String,
        #[clap(long)]
        secret: String,
        #[clap(long, default_value = "http://localhost:3000")]
        host: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

async fn main_inner() -> Result<()> {
    match Opt::parse().cmd {
        Cmd::Serve { bind } => serve(bind).await,
        Cmd::GenPair {} => {
            let mut rng = rand::thread_rng();
            let secret = SecretKey::random(&mut rng);
            let public = secret.public_key();
            println!("Public key: {public}");
            println!("Secret key: {}", secret.reveal_as_hex());
            Ok(())
        }
        Cmd::Broadcast {
            message,
            secret,
            host,
        } => broadcast(message, secret, host).await,
        Cmd::Sign { payload, secret } => {
            let secret = SecretKey::from_hex(&secret)?;
            let (signature, recovery) = secret.sign_recoverable(&payload)?;
            println!("Public key: {}", secret.public_key());
            println!("Signature: {signature}");
            println!("Recovery: {recovery:?}");
            Ok(())
        }
    }
}

async fn serve(bind: SocketAddr) -> Result<()> {
    const DB_PATH: &str = "example-cosmos-bridge.sqlite3";
    kolme::init_logger(true, None);
    let kolme = Kolme::new(SampleKolmeApp, DUMMY_CODE_VERSION, DB_PATH).await?;

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    set.spawn(processor.run());
    let listener = Listener::new(kolme.clone(), my_secret_key().clone());
    set.spawn(listener.run(ChainName::Cosmos));
    let approver = Approver::new(kolme.clone(), my_secret_key().clone());
    set.spawn(approver.run());
    let submitter = Submitter::new(
        kolme.clone(),
        SeedPhrase::from_str(SUBMITTER_SEED_PHRASE).unwrap(),
    );
    set.spawn(submitter.run());
    let api_server = ApiServer::new(kolme);
    set.spawn(api_server.run(bind));

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

async fn broadcast(message: String, secret: String, host: String) -> Result<()> {
    let message = serde_json::from_str::<SampleMessage>(&message)?;
    let secret = SecretKey::from_hex(&secret)?;
    let public = secret.public_key();
    let client = reqwest::Client::new();

    println!("Public sec1: {public}");
    println!("Public serialized: {}", serde_json::to_string(&public)?);

    #[derive(serde::Deserialize)]
    struct NonceResp {
        next_nonce: AccountNonce,
    }
    let NonceResp { next_nonce: nonce } = client
        .get(format!("{host}/get-next-nonce"))
        .query(&[("pubkey", public)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let signed = Transaction {
        pubkey: public,
        nonce,
        created: jiff::Timestamp::now(),
        messages: vec![Message::App(message)],
    }
    .sign(&secret)?;
    #[derive(serde::Deserialize)]
    struct Res {
        txhash: Sha256Hash,
    }
    let res = client
        .put(format!("{host}/broadcast"))
        .json(&signed)
        .send()
        .await?;

    if let Err(e) = res.error_for_status_ref() {
        let t = res.text().await?;
        anyhow::bail!("Error broadcasting:\n{e}\n{t}");
    }

    let Res { txhash } = res.json().await?;
    println!("txhash: {txhash}");
    Ok(())
}
