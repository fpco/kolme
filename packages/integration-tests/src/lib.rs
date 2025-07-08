pub mod setup;

use std::{
    path::PathBuf,
    sync::LazyLock,
};

use anyhow::Result;
use cosmos::{CodeId, Cosmos, CosmosNetwork, Wallet};
use shared::types::Sha256Hash;

static WASM_FILE: LazyLock<PathBuf> = LazyLock::new(|| {
    let mut file = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file.push("..");
    file.push("..");
    file.push("wasm");
    file.push("artifacts");
    file.push("kolme_cosmos_bridge.wasm");
    file.canonicalize().unwrap()
});

static WASM_CONTENTS: LazyLock<Vec<u8>> = LazyLock::new(|| std::fs::read(&*WASM_FILE).unwrap());

pub async fn prepare_local_contract(wallet: &Wallet) -> Result<CodeId> {
    let cosmos = get_cosmos_connection().await?;
    upload_contract(&cosmos, wallet).await
}

pub async fn get_cosmos_connection() -> Result<Cosmos> {
    const ATTEMPTS: u32 = 150;
    for i in 1..=ATTEMPTS {
        tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
        // Start a new Cosmos each time to avoid stale connections.
        let cosmos = CosmosNetwork::OsmosisLocal.connect().await?;
        if cosmos.get_latest_block_info().await.is_ok() {
            return Ok(cosmos);
        }
        if i % 10 == 0 {
            tracing::info!("Still trying to connect to Local Osmosis, attempt {i}/{ATTEMPTS}")
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
