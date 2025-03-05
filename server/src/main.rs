mod app;
mod cli;
mod prelude;

use clap::Parser;
use prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

async fn main_inner() -> Result<()> {
    dotenvy::dotenv()?;
    let opt = cli::Opt::parse();
    opt.init_logger();
    let app = App::new(opt).await?;
    Ok(())
}
