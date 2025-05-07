use anyhow::Result;
use clap::Parser;
use kolme::*;

use kademlia_discovery::{join_over_kademlia, validators, KademliaTestApp};

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
const DUMMY_CODE_VERSION: &str = "dummy code version";

async fn main_inner() -> Result<()> {
    match Opt::parse().cmd {
        Cmd::Validators { port } => {
            const DB_PATH: &str = "kademlia-test.fjall";

            kolme::init_logger(true, None);
            let kolme = Kolme::new(
                KademliaTestApp::default(),
                DUMMY_CODE_VERSION,
                KolmeStore::new_fjall(DB_PATH)?,
            )
            .await?;

            validators(kolme, port).await
        }
        Cmd::Client { validator } => {
            kolme::init_logger(true, None);
            let kolme = Kolme::new(
                KademliaTestApp::default(),
                DUMMY_CODE_VERSION,
                KolmeStore::new_in_memory(),
            )
            .await?;

            join_over_kademlia(kolme, &validator).await
        }
    }
}
