use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use example_six_sigma::{broadcast, serve};
use kolme::SecretKey;

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
        #[clap(long)]
        tx_log_path: Option<PathBuf>,
    },
    GenPair {},
    Broadcast {
        #[clap(long)]
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
        Cmd::Serve { bind, tx_log_path } => serve(bind, tx_log_path).await,
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
    }
}
