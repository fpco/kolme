use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[allow(dead_code)]
const BRIDGE_ARTIFACT_PATH: &str = "../../../contracts/ethereum/out/Bridge.sol/Bridge.json";

#[allow(dead_code)]
#[derive(Deserialize)]
struct FoundryArtifact {
    bytecode: ArtifactBytecode,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ArtifactBytecode {
    object: String,
}

#[allow(dead_code)]
fn bridge_artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BRIDGE_ARTIFACT_PATH)
}

#[allow(dead_code)]
fn parse_bridge_create_bytecode_str(json: &str) -> Result<Vec<u8>> {
    let artifact: FoundryArtifact =
        serde_json::from_str(json).context("failed to parse Bridge.json artifact")?;

    let object = artifact.bytecode.object.trim();
    anyhow::ensure!(
        !object.is_empty(),
        "Bridge.json artifact contains empty bytecode.object"
    );

    hex::decode(object.trim_start_matches("0x"))
        .context("Bridge.json artifact contains invalid hex in bytecode.object")
}

#[allow(dead_code)]
pub(super) fn load_bridge_create_bytecode() -> Result<Vec<u8>> {
    let path = bridge_artifact_path();
    let artifact = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read Ethereum bridge artifact at {}. Run `just build-ethereum-contract` first",
            path.display()
        )
    })?;

    parse_bridge_create_bytecode_str(&artifact)
}

#[cfg(test)]
mod tests {
    use super::parse_bridge_create_bytecode_str;

    #[test]
    fn parses_valid_foundry_artifact_bytecode() {
        let json = r#"{
            "bytecode": {
                "object": "0x60016002"
            }
        }"#;

        assert_eq!(
            parse_bridge_create_bytecode_str(json).unwrap(),
            vec![0x60, 0x01, 0x60, 0x02]
        );
    }

    #[test]
    fn rejects_missing_bytecode_object() {
        let json = r#"{
            "bytecode": {}
        }"#;

        let err = parse_bridge_create_bytecode_str(json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("failed to parse Bridge.json artifact"));
    }

    #[test]
    fn rejects_invalid_hex_bytecode() {
        let json = r#"{
            "bytecode": {
                "object": "0xnot-hex"
            }
        }"#;

        let err = parse_bridge_create_bytecode_str(json)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid hex"));
    }
}
