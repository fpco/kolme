use std::net::SocketAddr;

use anyhow::Result;
use clap::Parser;
use kolme::*;

use example_cosmos_bridge::{broadcast, serve, CosmosBridgeApp};

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
        Cmd::Serve { bind } => {
            const DB_PATH: &str = "example-cosmos-bridge.fjall";
            const DUMMY_CODE_VERSION: &str = "dummy code version";

            kolme::init_logger(true, None);
            let kolme = Kolme::new(
                CosmosBridgeApp,
                DUMMY_CODE_VERSION,
                KolmeStore::new_fjall(DB_PATH)?,
            )
            .await?;

            serve(kolme, bind).await
        }
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
            let SignatureWithRecovery { recid, sig } = secret.sign_recoverable(&payload)?;
            println!("Public key: {}", secret.public_key());
            println!("Signature: {sig}");
            println!("Recovery: {recid:?}");
            Ok(())
        }
    }
}
