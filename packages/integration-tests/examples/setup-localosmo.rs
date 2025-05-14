use std::str::FromStr;

use anyhow::{Context, Result};
use cosmos::{Cosmos, CosmosNetwork, HasAddressHrp, SeedPhrase};
use kolme::Sha256Hash;

#[tokio::main]
async fn main() -> Result<()> {
    main_inner().await
}

const WASM_FILE: &str = "../../artifacts/kolme_cosmos_bridge.wasm";

async fn main_inner() -> Result<()> {
    println!("Building contracts and launching chain");
    let (current_contract, cosmos) = tokio::try_join!(build_contracts(), launch_local_osmo())?;

    println!("Chain is running, checking for existing contract...");

    match cosmos.make_code_id(1).download().await {
        Ok(contract) => {
            println!("Existing contract for code ID #1 discovered");
            let uploaded = Sha256Hash::hash(&contract);
            if uploaded == current_contract {
                println!("Contract matches our code, exiting");
                return Ok(());
            }
            println!("Contract didn't match, restarting chain");
        }
        Err(e) => {
            println!("Error loading code ID #1, restarting chain: {e}");
        }
    }
    stop_local_osmo().await?;
    let cosmos = launch_local_osmo().await?;
    let wallet = SeedPhrase::from_str("osmosis-local")?
        .with_hrp(CosmosNetwork::OsmosisLocal.get_address_hrp())?;
    let code_id = cosmos
        .store_code_path(&wallet, WASM_FILE)
        .await?
        .get_code_id();
    anyhow::ensure!(code_id == 1);
    Ok(())
}

async fn build_contracts() -> Result<Sha256Hash> {
    tokio::process::Command::new("just")
        .arg("build-contracts")
        .spawn()?
        .wait()
        .await
        .map(|_| ())
        .context("Error while building contracts")?;
    let contents = tokio::fs::read(WASM_FILE).await?;
    Ok(Sha256Hash::hash(&contents))
}

async fn launch_local_osmo() -> Result<Cosmos> {
    let cosmos = CosmosNetwork::OsmosisLocal.connect().await?;
    if cosmos.get_latest_block_info().await.is_ok() {
        return Ok(cosmos);
    }

    tokio::process::Command::new("docker")
        .arg("compose")
        .arg("up")
        .arg("-d")
        .spawn()?
        .wait()
        .await
        .map(|_| ())
        .context("Error while launching Docker Compose")?;
    const ATTEMPTS: u32 = 150;
    for i in 1..=ATTEMPTS {
        tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
        // Start a new Cosmos each time to avoid stale connections.
        let cosmos = CosmosNetwork::OsmosisLocal.connect().await?;
        if cosmos.get_latest_block_info().await.is_ok() {
            return Ok(cosmos);
        }
        if i % 10 == 0 {
            println!("Still trying to connect to Local Osmosis, attempt {i}/{ATTEMPTS}")
        }
    }
    Err(anyhow::anyhow!(
        "Could not establish a connection to Osmosis Local"
    ))
}

async fn stop_local_osmo() -> Result<()> {
    tokio::process::Command::new("docker")
        .arg("compose")
        .arg("kill")
        .spawn()?
        .wait()
        .await
        .map(|_| ())
        .context("Error while killing Docker Compose")?;
    tokio::process::Command::new("docker")
        .arg("compose")
        .arg("rm")
        .arg("-f")
        .spawn()?
        .wait()
        .await
        .map(|_| ())
        .context("Error while killing Docker Compose")?;
    Ok(())
}
