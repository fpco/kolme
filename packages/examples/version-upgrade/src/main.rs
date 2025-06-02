use clap::Parser;
use version_upgrade::processor;

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
async fn main() {
    match Opt::parse().cmd {
        Cmd::Processor => processor().await,
    }
}
