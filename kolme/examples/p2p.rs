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
}

// Another keypair for client testing:
// Public key: 02c2b386e42945d4c11712a5bc1d20d085a7da63e57c214e2742a684a97d436599
// Secret key: 127831b9459b538eab9a338b1e96fc34249a5154c96180106dd87d39117e8e02

const SECRET_KEY_HEX: &str = "bd9c12efb8c473746404dfd893dd06ad8e62772c341d5de9136fec808c5bed92";

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
        GenesisInfo {
            kolme_ident: "p2p example".to_owned(),
            processor: my_public_key,
            listeners: set.clone(),
            needed_listeners: 1,
            approvers: set,
            needed_approvers: 1,
            chains: BTreeMap::new(),
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
        }
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
    /// Run a node with the processor. Does not include any API server.
    Processor {},
    /// Run a node with just basic API server capabilities
    ApiServer {
        #[clap(long, default_value = "[::]:3000")]
        bind: SocketAddr,
    },
    /// Send a say hi message over the API server
    SayHi {
        /// Secret key, auto-generated if not provided.
        #[clap(long)]
        secret: Option<String>,
        /// Hostname of the API server
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
        Cmd::Processor {} => processor().await,
        Cmd::ApiServer { bind } => api_server(bind).await,
        Cmd::SayHi { secret, host } => say_hi(secret, host).await,
    }
}

async fn processor() -> Result<()> {
    const DB_PATH: &str = "example-p2p-processor.sqlite3";
    kolme::init_logger(true, None);
    let kolme = Kolme::new(SampleKolmeApp, DUMMY_CODE_VERSION, DB_PATH).await?;

    let mut set = JoinSet::new();

    let processor = Processor::new(kolme.clone(), my_secret_key().clone());
    set.spawn(processor.run());
    let gossip = Gossip::new(kolme).await?;
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

async fn api_server(bind: SocketAddr) -> Result<()> {
    const DB_PATH: &str = "example-p2p-api-server.sqlite3";
    kolme::init_logger(true, None);
    let kolme = Kolme::new(SampleKolmeApp, DUMMY_CODE_VERSION, DB_PATH).await?;

    let mut set = JoinSet::new();

    let gossip = Gossip::new(kolme.clone()).await?;
    set.spawn(gossip.run());
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

async fn say_hi(secret: Option<String>, host: String) -> Result<()> {
    let secret = match secret {
        Some(secret) => SecretKey::from_hex(&secret)?,
        None => SecretKey::random(&mut rand::thread_rng()),
    };
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
        messages: vec![Message::App(SampleMessage::SayHi {})],
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
