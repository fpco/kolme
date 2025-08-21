use anyhow::Result;
use clap::Parser;

use kademlia_discovery::{
    client, invalid_client, new_node_client, new_version_node, observer_node, validators,
};
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
        /// Fjall storage
        #[clap(long)]
        use_fjall_storage: bool,
        /// Start Upgrade process
        #[clap(long)]
        start_upgrade: bool,
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
    /// Run observer node with API at given port
    Observer {
        /// Address to connect to validators on
        #[clap(long)]
        validator: String,
        /// API server port
        #[clap(long, default_value_t = 2005)]
        api_server_port: u16,
    },
    /// Run node with API at given port
    NewVersionNode {
        /// API server port
        #[clap(long, default_value_t = 2003)]
        api_server_port: u16,
    },
    /// Client proposing txs to new node
    NewVersionClient {
        /// Address to connect to validators on
        #[clap(long)]
        validator: String,
        /// Run continously by proposing new txs
        #[clap(long)]
        continous: bool,
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
            start_upgrade,
            use_fjall_storage,
        } => validators(port, enable_api_server, start_upgrade, use_fjall_storage).await,
        Cmd::Client {
            validator,
            continous,
        } => client(&validator, SecretKey::random(), continous).await,
        Cmd::Observer {
            validator,
            api_server_port,
        } => observer_node(&validator, api_server_port).await,
        Cmd::InvalidClient { validator } => invalid_client(&validator).await,
        Cmd::NewVersionNode { api_server_port } => new_version_node(api_server_port).await,
        Cmd::NewVersionClient {
            validator,
            continous,
        } => new_node_client(&validator, SecretKey::random(), continous).await,
    }
}
