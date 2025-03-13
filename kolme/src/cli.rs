use std::path::PathBuf;

use tracing::Level;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[derive(clap::Parser)]
pub struct Opt {
    /// Enable verbose output?
    #[clap(long, short)]
    verbose: bool,
    /// Location for data storage
    #[clap(long, default_value = "local-test.sqlite3")]
    pub storage: PathBuf,
}

impl Opt {
    pub fn init_logger(&self) {
        let env_directive = if self.verbose {
            format!("{}=debug,info", env!("CARGO_CRATE_NAME"))
                .parse()
                .unwrap()
        } else {
            Level::INFO.into()
        };

        tracing_subscriber::registry()
            .with(
                fmt::Layer::default()
                    .log_internal_errors(true)
                    .and_then(EnvFilter::from_default_env().add_directive(env_directive)),
            )
            .init();
        tracing::info!("Initialized Logging");
    }
}
