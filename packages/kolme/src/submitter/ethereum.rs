use std::{collections::BTreeMap, fs, path::PathBuf};

use alloy::{
    network::TransactionBuilder,
    primitives::Address,
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolConstructor,
};
use anyhow::Context;
use serde::Deserialize;

use base64::Engine;

use crate::{EthereumChain, PublicKey, SignatureWithRecovery, ValidatorSet};

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

    #[sol(rpc)]
    interface BridgeExecute {
        function execute_signed(
            bytes payload,
            bytes processor,
            bytes[] approvers
        ) external;
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

fn build_bridge_initcode_with_create_bytecode(
    create_bytecode: &[u8],
    validator_set: &ValidatorSet,
) -> Vec<u8> {
    let mut initcode = create_bytecode.to_vec();
    initcode.extend_from_slice(&validator_set_constructor_args(validator_set).abi_encode());
    initcode
}

fn build_bridge_initcode(validator_set: &ValidatorSet) -> anyhow::Result<Vec<u8>> {
    Ok(build_bridge_initcode_with_create_bytecode(
        &load_bridge_create_bytecode()?,
        validator_set,
    ))
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

/// Converts Kolme internal ECDSA signature format into Ethereum expected wire
/// format (r||s||v, v=27/28)
fn to_ethereum_signature(sig: SignatureWithRecovery) -> anyhow::Result<Vec<u8>> {
    let mut out = sig.sig.to_bytes();
    let recid = sig.recid.to_byte();
    anyhow::ensure!(
        recid <= 1,
        "Invalid Ethereum recovery id {recid}, expected 0 or 1"
    );
    out.push(recid + 27);
    Ok(out)
}

pub(super) async fn execute(
    chain: EthereumChain,
    signer: PrivateKeySigner,
    contract: &str,
    processor: SignatureWithRecovery,
    approvals: &BTreeMap<PublicKey, SignatureWithRecovery>,
    payload_b64: &str,
) -> anyhow::Result<String> {
    let payload = base64::engine::general_purpose::STANDARD
        .decode(payload_b64)
        .context("Failed to decode Ethereum bridge action payload from base64")?;

    let processor = to_ethereum_signature(processor)?;
    let approvers = approvals
        .values()
        .copied()
        .map(to_ethereum_signature)
        .collect::<anyhow::Result<Vec<_>>>()?;

    let url = reqwest::Url::parse(chain.default_rpc_url())
        .with_context(|| format!("Invalid default Ethereum RPC URL for {chain:?}"))?;
    let provider = ProviderBuilder::new().wallet(signer).connect_http(url);
    let address: Address = contract.parse()?;
    let bridge = BridgeExecute::new(address, provider);

    let pending = bridge
        .execute_signed(
            payload.into(),
            processor.into(),
            approvers.into_iter().map(Into::into).collect(),
        )
        .send()
        .await?;
    let tx_hash = *pending.tx_hash();
    pending.get_receipt().await?;

    Ok(format!("{tx_hash:#x}"))
}

#[cfg(test)]
mod tests {
    use shared::cryptography::RecoveryId;

    use alloy::sol_types::SolConstructor;

    use super::{
        build_bridge_initcode_with_create_bytecode, parse_bridge_create_bytecode_str,
        to_ethereum_signature, validator_set_constructor_args,
    };
    use crate::{SecretKey, SignatureWithRecovery, ValidatorSet};

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
        let initcode = build_bridge_initcode_with_create_bytecode(&[0x60, 0x01], &set);
        assert!(initcode.starts_with(&[0x60, 0x01]));
        assert!(initcode.ends_with(&args));
    }

    #[test]
    fn ethereum_signature_uses_rsv_with_27_plus_recid() {
        let key = SecretKey::random();
        let sig = key.sign_recoverable(b"msg").unwrap();

        let encoded = to_ethereum_signature(sig).unwrap();
        assert_eq!(encoded.len(), 65);
        assert!(encoded[64] == 27 || encoded[64] == 28);
    }

    #[test]
    fn ethereum_signature_rejects_invalid_recovery_id() {
        let sig = SignatureWithRecovery {
            recid: RecoveryId::from_byte(2).unwrap(),
            sig: SecretKey::random().sign_recoverable(b"msg").unwrap().sig,
        };
        let err = to_ethereum_signature(sig).unwrap_err().to_string();
        assert!(err.contains("Invalid Ethereum recovery id 2"));
    }
}
