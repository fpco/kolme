use anyhow::Result;
use clap::Parser;
use version_upgrade::nodes::{client::client, processor::processor};

#[derive(Parser)]
enum Cmd {
    Processor {
        /// run as bootstrap node, which can be connected to (port 4546)
        #[arg(long, default_value_t = false)]
        bootstrap: bool,
    },
    Client,
}

#[derive(Parser)]
struct Opt {
    #[clap(subcommand)]
    cmd: Cmd,
    #[arg(long, default_value_t = String::from("1"))]
    version: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    kolme::init_logger(true, None);
    let opts = Opt::parse();

    match opts.cmd {
        Cmd::Processor { bootstrap } => processor(bootstrap, &opts.version).await?,
        Cmd::Client => client(&opts.version).await?,
    }
    Ok(())
}
