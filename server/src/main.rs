mod app;
mod cli;
mod framework_state;
mod kolme;
mod prelude;
mod sample;
mod types;

use clap::Parser;
use prelude::*;
use sample::SampleKolmeApp;

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

async fn main_inner() -> Result<()> {
    dotenvy::dotenv()?;
    let opt = cli::Opt::parse();
    opt.init_logger();
    let app = Kolme::new(SampleKolmeApp, "Dev code", &opt.storage).await?;
    Ok(())
}
