use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use example_six_sigma::{broadcast, serve, state, AppComponent, SixSigmaApp, StoreType};
use kolme::SecretKey;

#[derive(clap::Parser)]
struct Opt {
    #[clap(long)]
    db_path: Option<PathBuf>,
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Parser)]
enum Cmd {
    Serve {
        #[command(subcommand)]
        component: Option<AppComponent>,
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
    State {},
}

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

const DB_PATH: &str = "six-sigma-app.fjall";

async fn main_inner() -> Result<()> {
    let opt = Opt::parse();
    let db_path = opt.db_path.unwrap_or(DB_PATH.into());
    match opt.cmd {
        Cmd::Serve {
            bind,
            tx_log_path,
            component,
        } => {
            kolme::init_logger(true, None);
            serve(
                SixSigmaApp::new_cosmos(),
                bind,
                StoreType::Fjall(db_path),
                tx_log_path,
                component,
            )
            .await
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
        } => {
            broadcast(message, secret, host).await?;
            Ok(())
        }
        Cmd::State {} => {
            let state = state(SixSigmaApp::new_cosmos(), db_path).await?;
            println!("{}", serde_json::to_string(&state)?);
            Ok(())
        }
    }
}
