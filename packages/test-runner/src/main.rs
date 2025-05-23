mod join_set;

use anyhow::{Context, Result};
use join_set::try_join;

const DOCKER_COMPOSE_DIR: &str = "packages/integration-tests";

fn main() -> Result<()> {
    println!("Killing Kademlia validators Docker container (if running)");
    kill_kademlia_validators();
    let _kill_kademlia = KillKademliaOnDrop;

    println!("In parallel: building tests, building contracts, launching local Osmosis");
    try_join(|s| {
        s.spawn(build_tests);
        s.spawn(build_contracts);
        s.spawn(launch_local_osmo);
    })?;

    println!("Launching Kademlia validators Docker container");
    launch_kademlia_validators()?;

    println!("Running test suite");
    run_test_suite()?;

    println!("Running Kademlia test case");
    run_kademlia_test()?;
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

struct KillKademliaOnDrop;

impl Drop for KillKademliaOnDrop {
    fn drop(&mut self) {
        kill_kademlia_validators();
    }
}

// We do this on a "best effort" basis. Always succeed.
fn kill_kademlia_validators() {
    if let Err(e) = kill_kademlia_validators_inner() {
        eprintln!("Error killing Kademlia validators: {e:?}");
    }
}

fn kill_kademlia_validators_inner() -> Result<()> {
    let status = std::process::Command::new("docker")
        .arg("compose")
        .arg("stop")
        .arg("kademlia-validators")
        .current_dir(DOCKER_COMPOSE_DIR)
        .spawn()?
        .wait()
        .context("Error while launching Docker Compose")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Kill Kademlia validators failed with exit status: {status}"
        ))
    }
}

fn launch_kademlia_validators() -> Result<()> {
    let status = std::process::Command::new("docker")
        .arg("compose")
        .arg("up")
        .arg("-d")
        .arg("kademlia-validators")
        .current_dir(DOCKER_COMPOSE_DIR)
        .spawn()?
        .wait()
        .context("Error while launching Docker Compose")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Launch Kademlia validators failed with exit status: {status}"
        ))
    }
}

fn run_kademlia_test() -> Result<()> {
    let status = std::process::Command::new("docker")
        .arg("compose")
        .arg("run")
        .arg("--rm")
        .arg("kademlia-client")
        .current_dir(DOCKER_COMPOSE_DIR)
        .spawn()?
        .wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Running Kademlia test failed with exit status: {status}"
        ))
    }
}
