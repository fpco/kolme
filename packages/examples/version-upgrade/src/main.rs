use anyhow::Result;
use clap::Parser;
use version_upgrade::nodes::{client::client, processor::processor};

#[derive(Parser)]
enum Cmd {
    Processor,
    Client,
}

#[derive(Parser)]
struct Opt {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[tokio::main]
async fn main() -> Result<()> {
    kolme::init_logger(true, None);
    match Opt::parse().cmd {
        Cmd::Processor => processor().await?,
        Cmd::Client => client().await?,
    }
    Ok(())
}
