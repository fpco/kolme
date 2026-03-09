use std::{fs, path::PathBuf};

use alloy::{
    network::TransactionBuilder,
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolConstructor,
};
use anyhow::Context;
use serde::Deserialize;

use crate::{EthereumChain, ValidatorSet};

const BRIDGE_ARTIFACT_PATH: &str = "../../contracts/ethereum/out/Bridge.sol/Bridge.json";

#[derive(Deserialize)]
struct FoundryArtifact {
    bytecode: ArtifactBytecode,
}

#[derive(Deserialize)]
struct ArtifactBytecode {
    object: String,
}

sol! {
    contract Bridge {
        constructor(
            bytes processor,
            bytes[] listeners,
            uint16 neededListeners,
            bytes[] approvers,
            uint16 neededApprovers
        );
    }
}

fn bridge_artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BRIDGE_ARTIFACT_PATH)
}

fn parse_bridge_create_bytecode_str(json: &str) -> anyhow::Result<Vec<u8>> {
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

pub(super) fn load_bridge_create_bytecode() -> anyhow::Result<Vec<u8>> {
    let path = bridge_artifact_path();
    let artifact = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read Ethereum bridge artifact at {}. Run `just build-ethereum-contract` first",
            path.display()
        )
    })?;

    parse_bridge_create_bytecode_str(&artifact)
}

fn validator_set_constructor_args(validator_set: &ValidatorSet) -> Bridge::constructorCall {
    Bridge::constructorCall::new((
        validator_set.processor.as_bytes().into_vec().into(),
        validator_set
            .listeners
            .iter()
            .copied()
            .map(|key| key.as_bytes().into_vec().into())
            .collect(),
        validator_set.needed_listeners,
        validator_set
            .approvers
            .iter()
            .copied()
            .map(|key| key.as_bytes().into_vec().into())
            .collect(),
        validator_set.needed_approvers,
    ))
}

fn build_bridge_initcode(validator_set: &ValidatorSet) -> anyhow::Result<Vec<u8>> {
    let mut initcode = load_bridge_create_bytecode()?;
    initcode.extend_from_slice(&validator_set_constructor_args(validator_set).abi_encode());
    Ok(initcode)
}

pub(super) async fn instantiate(
    chain: EthereumChain,
    signer: PrivateKeySigner,
    validator_set: &ValidatorSet,
) -> anyhow::Result<String> {
    let url = reqwest::Url::parse(chain.default_rpc_url())
        .with_context(|| format!("Invalid default Ethereum RPC URL for {chain:?}"))?;
    let provider = ProviderBuilder::new().wallet(signer).connect_http(url);
    let receipt = provider
        .send_transaction(
            TransactionRequest::default().with_deploy_code(build_bridge_initcode(validator_set)?),
        )
        .await?
        .get_receipt()
        .await?;
    let address = receipt
        .contract_address
        .context("Ethereum deployment transaction did not return a contract address")?;
    Ok(format!("{address:#x}"))
}

#[cfg(test)]
mod tests {
    use alloy::sol_types::SolConstructor;

    use super::{
        build_bridge_initcode, parse_bridge_create_bytecode_str, validator_set_constructor_args,
    };
    use crate::{SecretKey, ValidatorSet};

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

    #[test]
    fn constructor_encoding_uses_validator_set_bytes() {
        let processor = SecretKey::random().public_key();
        let listener = SecretKey::random().public_key();
        let approver = SecretKey::random().public_key();
        let set = ValidatorSet {
            processor,
            listeners: [listener].into_iter().collect(),
            needed_listeners: 1,
            approvers: [approver].into_iter().collect(),
            needed_approvers: 1,
        };

        let encoded = validator_set_constructor_args(&set).abi_encode();
        assert!(encoded
            .windows(33)
            .any(|window| window == processor.as_bytes().as_ref()));
        assert!(encoded
            .windows(33)
            .any(|window| window == listener.as_bytes().as_ref()));
        assert!(encoded
            .windows(33)
            .any(|window| window == approver.as_bytes().as_ref()));
    }

    #[test]
    fn initcode_contains_constructor_args_suffix() {
        let processor = SecretKey::random().public_key();
        let set = ValidatorSet {
            processor,
            listeners: [processor].into_iter().collect(),
            needed_listeners: 1,
            approvers: [processor].into_iter().collect(),
            needed_approvers: 1,
        };

        let args = validator_set_constructor_args(&set).abi_encode();
        let initcode = build_bridge_initcode(&set).unwrap();
        assert!(initcode.ends_with(&args));
    }
}
