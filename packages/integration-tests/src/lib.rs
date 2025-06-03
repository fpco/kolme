pub mod setup;

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::Result;
use cosmos::{CodeId, Cosmos, CosmosNetwork, Wallet};
use shared::types::Sha256Hash;

static DOCKER_COMPOSE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap()
});

static WASM_FILE: LazyLock<PathBuf> = LazyLock::new(|| {
    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("..");
    file.push("..");
    file.push("artifacts");
    file.push("kolme_cosmos_bridge.wasm");
    file.canonicalize().unwrap()
});

static WASM_CONTENTS: LazyLock<Vec<u8>> = LazyLock::new(|| std::fs::read(&*WASM_FILE).unwrap());

pub async fn prepare_local_contract(wallet: &Wallet) -> Result<CodeId> {
    ensure_docker_compose_running().await?;
    let cosmos = get_cosmos_connection().await?;
    upload_contract(&cosmos, wallet).await
}

async fn ensure_docker_compose_running() -> Result<()> {
    let output = tokio::process::Command::new("docker")
        .args(["compose", "ls", "--format", "json"])
        .current_dir(&*DOCKER_COMPOSE_DIR)
        .output()
        .await?;
    anyhow::ensure!(
        output.status.success(),
        "Could not check if Docker Compose is running, got status: {}.\nstdout:\n\n{}\n\nstderr:\n\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let serde_json::Value::Array(values) = serde_json::from_slice(&output.stdout)? else {
        anyhow::bail!("Non-JSON output from docker compose ls --format json");
    };

    if values.is_empty() {
        let status = tokio::process::Command::new("docker")
            .args(["compose", "up", "-d"])
            .current_dir(&*DOCKER_COMPOSE_DIR)
            .spawn()?
            .wait()
            .await?;
        anyhow::ensure!(
            status.success(),
            "docker compose up exited with status {status}"
        );
    }

    Ok(())
}

async fn get_cosmos_connection() -> Result<Cosmos> {
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

pub async fn upload_contract(cosmos: &Cosmos, wallet: &Wallet) -> Result<CodeId> {
    let hash = Sha256Hash::hash(&*WASM_CONTENTS);

    // Fun optimization: if we already uploaded, use it.
    // Since we expected queries to be cheap and a very small number of uploads,
    // the linear search is worth it.
    if let Ok(code_id) = find_code_id_by_hash(cosmos, hash).await {
        return Ok(code_id);
    }

    cosmos
        .store_code(wallet, WASM_CONTENTS.clone(), Some(WASM_FILE.clone()))
        .await
        .map_err(anyhow::Error::from)
}

async fn find_code_id_by_hash(cosmos: &Cosmos, expected: Sha256Hash) -> Result<CodeId> {
    for id in 1..200 {
        let code_id = cosmos.make_code_id(id);
        let actual = Sha256Hash::hash(&code_id.download().await?);
        if actual == expected {
            return Ok(code_id);
        }
    }
    Err(anyhow::anyhow!("Did not find code ID with hash {expected}"))
}
