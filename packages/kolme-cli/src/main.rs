pub(crate) mod chain_query;

use anyhow::{Context, Result};
use chain_query::ChainApi;
use clap::Parser;
use comfy_table::{presets, Cell, Table};
use kolme::*;
use reqwest::{RequestBuilder, Url};

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

/// Command line helper to interact with Kolme chains.
#[derive(clap::Parser)]
enum Cmd {
    /// Generate a new keypair.
    GenKeypair {},
    /// Print public key
    PubKey {
        #[clap(long, env = "KOLME_CLI_SECRET_KEY")]
        secret: SecretKey,
    },
    /// Send a transaction via an API server.
    SendTx(SendTxOpt),
    /// Fork height information
    ForkHeight {
        /// Chain version tag that you want to query
        #[clap(long, env = "KOLME_CLI_CHAIN_VERSION")]
        version: String,
        /// API server root
        #[clap(
            long,
            env = "KOLME_CLI_API_SERVER",
            default_value = "http://localhost:3000"
        )]
        api_server: Url,
    },
}

#[derive(clap::Parser)]
struct SendTxOpt {
    /// API server root
    #[clap(long, env = "API_SERVER", default_value = "http://localhost:3000")]
    api_server: Url,
    /// Signing private key
    #[clap(long, env = "SECRET_KEY")]
    secret_key: SecretKey,
    /// JSON messages to send
    #[clap(required = true)]
    messages: Vec<Message<serde_json::Value>>,
}

async fn main_inner() -> Result<()> {
    match Cmd::parse() {
        Cmd::GenKeypair {} => gen_keypair(),
        Cmd::SendTx(opt) => send_tx(opt).await?,
        Cmd::PubKey { secret } => {
            let public = secret.public_key();
            eprintln!("Public key: {public}");
        }
        Cmd::ForkHeight {
            version,
            api_server,
        } => {
            let app = ChainApi::new(api_server)?;
            let fork_info = app.fork_info(version).await?;

            let mut table = Table::new();
            table.load_preset(presets::NOTHING);
            table
                .add_row(vec![
                    Cell::new("First block"),
                    Cell::new(fork_info.first_block.0),
                ])
                .add_row(vec![
                    Cell::new("Last block"),
                    Cell::new(fork_info.last_block.0),
                ]);

            println!("{table}");
        }
    }
    Ok(())
}

fn gen_keypair() {
    let secret = SecretKey::random();
    let public = secret.public_key();
    println!("Public key: {public}");
    println!("Secret key: {}", secret.reveal_as_hex());
}

trait RequestBuilderExt {
    async fn send_check_json<T: serde::de::DeserializeOwned>(self) -> Result<T>;
}

impl RequestBuilderExt for RequestBuilder {
    async fn send_check_json<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        let res = self.send().await?;
        match res.error_for_status_ref() {
            Ok(_) => res.json().await.map_err(anyhow::Error::from),
            Err(e) => {
                let body = res.text().await?;
                Err(e).context(body)
            }
        }
    }
}

async fn send_tx(opt: SendTxOpt) -> Result<()> {
    let SendTxOpt {
        api_server,
        secret_key,
        messages,
    } = opt;

    let client = reqwest::ClientBuilder::new().build()?;

    let pubkey = secret_key.public_key();

    #[derive(serde::Deserialize)]
    struct NextNonceRes {
        next_nonce: AccountNonce,
    }
    let NextNonceRes { next_nonce: nonce } = client
        .get(api_server.join("get-next-nonce")?)
        .query(&[("pubkey", pubkey)])
        .send_check_json()
        .await?;

    let transaction = Transaction {
        pubkey,
        nonce,
        created: jiff::Timestamp::now(),
        messages,
        max_height: None,
    }
    .sign(&secret_key)?;

    println!(
        "Broadcasting transaction:\n{}\n",
        serde_json::to_string(&transaction)?
    );

    let res: serde_json::Value = client
        .put(api_server.join("broadcast")?)
        .json(&transaction)
        .send_check_json()
        .await?;
    println!("{res:#?}");

    Ok(())
}
