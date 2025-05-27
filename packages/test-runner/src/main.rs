mod join_set;

use anyhow::{Context, Result};
use join_set::try_join;

const DOCKER_COMPOSE_DIR: &str = "packages/integration-tests";

fn main() -> Result<()> {
    println!("In parallel: building tests, building contracts, launching local Osmosis");
    try_join(|s| {
        s.spawn(build_tests);
        s.spawn(build_contracts);
        s.spawn(launch_local_osmo);
    })?;

    println!("Running test suite");
    run_test_suite()?;
    Ok(())
}

fn build_tests() -> Result<()> {
    (|| {
        let status = std::process::Command::new("cargo")
            .arg("build")
            .arg("--release")
            .arg("--workspace")
            .arg("--all-targets")
            .spawn()?
            .wait()
            .context("Error while building tests")?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Failure status code for build_tests: {status}"
            ))
        }
    })()
    .context("Failure while building test executables")
}

fn build_contracts() -> Result<()> {
    (|| {
        let status = std::process::Command::new("just")
            .arg("build-contracts")
            .spawn()?
            .wait()?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Failure status code for build_contracts: {status}"
            ))
        }
    })()
    .context("Failure while building contracts")
}

fn launch_local_osmo() -> Result<()> {
    (|| {
        let status = std::process::Command::new("docker")
            .arg("compose")
            .arg("up")
            .arg("-d")
            .arg("localosmosis")
            .arg("postgres")
            .current_dir(DOCKER_COMPOSE_DIR)
            .spawn()?
            .wait()
            .context("Error while launching Docker Compose")?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Error while launching local osmosis, got status code: {status}"
            ))
        }
    })()
    .context("Error while launching local osmo")
}

fn run_test_suite() -> Result<()> {
    (|| {
        let status = std::process::Command::new("cargo")
            .arg("test")
            .arg("--release")
            .arg("--workspace")
            .env("RUST_BACKTRACE", "1")
            .env(
                "PROCESSOR_BLOCK_DB",
                "psql://postgres:postgres@localhost:45921/postgres",
            )
            .spawn()?
            .wait()
            .context("Error while running test suite")?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Running test suite failed with exit status: {status}"
            ))
        }
    })()
    .context("Error while running test suite")
}
