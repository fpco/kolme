use anyhow::Result;
use clap::Parser;
use version_upgrade::nodes::processor::processor;

#[derive(Parser)]
enum Cmd {
    Processor,
}

#[derive(Parser)]
struct Opt {
    #[clap(subcommand)]
    cmd: Cmd,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Opt::parse().cmd {
        Cmd::Processor => processor().await?,
    }
    Ok(())
}
