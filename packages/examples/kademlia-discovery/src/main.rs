use anyhow::Result;
use clap::Parser;

use kademlia_discovery::{client, invalid_client, observer_node, validators};
use kolme::SecretKey;

#[derive(clap::Parser)]
struct Opt {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Parser)]
enum Cmd {
    /// Run the validator set
    Validators {
        /// Port for Gossip to listen on
        port: u16,
        /// Run api server at 2002 port
        #[clap(long)]
        enable_api_server: bool,
    },
    /// Run a test of connecting over Kademlia
    Client {
        /// Address to connect to validators on
        #[clap(long)]
        validator: String,
        /// Run continously by proposing new txs
        #[clap(long)]
        continous: bool,
    },
    /// Run observer node with API at 2005 port
    Observer {
        /// Address to connect to validators on
        #[clap(long)]
        validator: String,
    },
    /// Invalid client
    InvalidClient {
        /// Address to connect to validators on
        #[clap(long)]
        validator: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

async fn main_inner() -> Result<()> {
    kolme::init_logger(true, None);
    match Opt::parse().cmd {
        Cmd::Validators {
            port,
            enable_api_server,
        } => validators(port, enable_api_server).await,
        Cmd::Client {
            validator,
            continous,
        } => {
            client(
                &validator,
                SecretKey::random(&mut rand::thread_rng()),
                continous,
            )
            .await
        }
        Cmd::Observer { validator } => observer_node(&validator).await,
        Cmd::InvalidClient { validator } => invalid_client(&validator).await,
    }
}
