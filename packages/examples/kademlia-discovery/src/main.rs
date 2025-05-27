use anyhow::Result;
use clap::Parser;

use kademlia_discovery::{client, validators};

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
    },
    /// Run a test of connecting over Kademlia
    Client {
        /// Address to connect to validators on
        validator: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

async fn main_inner() -> Result<()> {
    match Opt::parse().cmd {
        Cmd::Validators { port } => validators(port).await,
        Cmd::Client { validator } => client(&validator).await,
    }
}
