use std::net::SocketAddr;

use anyhow::Result;
use clap::Parser;

use example_p2p::{api_server, processor, say_hi};

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
