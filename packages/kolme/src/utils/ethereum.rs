//! Ethereum-specific helpers.
#![cfg(feature = "ethereum")]

use std::str::FromStr;

use alloy::{
    primitives::{Address, Bytes, U256},
    sol,
    sol_types::{SolCall, SolValue},
};

use crate::SignatureWithRecovery;
use crate::{PublicKey, ValidatorSet, ValidatorType};

// Keep these in sync with action constants in contracts/ethereum/src/BridgeActions.sol.
pub const ACTION_EXECUTE: u8 = 0;
pub const ACTION_SELF_REPLACE: u8 = 1;
pub const ACTION_NEW_SET: u8 = 2;
pub const ETH_NATIVE_DENOM: &str = "eth";

sol! {
    struct ExecuteAction {
        address target;
        uint256 value;
        bytes data;
    }

    struct SelfReplaceAction {
        uint8 validatorType;
        bytes current;
        bytes replacement;
    }

    struct NewSetAction {
        bytes processor;
        bytes[] listeners;
        uint16 neededListeners;
        bytes[] approvers;
        uint16 neededApprovers;
        bytes rendered;
        bytes[] approvals;
    }

    interface IERC20 {
        function transfer(address recipient, uint256 amount) external returns (bool);
    }
}

/// ABI-encode execute_signed() payload `(uint64 action_id, bytes action_data)` according
/// to Solidity ABI rules.
pub fn abi_encode_u64_and_bytes(value: u64, data: &[u8]) -> Vec<u8> {
    (value, Bytes::copy_from_slice(data)).abi_encode_params()
}

/// ABI-encode _executeAction() payload `(uint8, bytes)` in bridge contract
pub fn abi_encode_u8_and_bytes(value: u8, data: &[u8]) -> Vec<u8> {
    // In ABI head encoding, uint8 and uint256 both occupy one 32-byte word;
    // for values 0..=255 the byte layout is identical.
    (U256::from(value), Bytes::copy_from_slice(data)).abi_encode_params()
}

fn encode_execute_action(target: Address, value: U256, data: Vec<u8>) -> Vec<u8> {
    // Solidity decodes execute data as tuple: (address,uint256,bytes).
    // Encode as tuple directly to match abi.decode(data, (address,uint256,bytes)).
    let action = (target, value, Bytes::from(data)).abi_encode_params();
    abi_encode_u8_and_bytes(ACTION_EXECUTE, &action)
}

pub fn evm_address_to_string(address: Address) -> String {
    format!("{:#x}", address)
}

pub fn normalize_evm_address(address: &str) -> anyhow::Result<String> {
    let address = Address::from_str(address)?;
    Ok(evm_address_to_string(address))
}

/// `denom` could be "eth" (case-insensitive) or a EVM address.
/// Will be canonicalized (lowercase "eth" or "0x...")
pub fn normalize_ethereum_denom(denom: &str) -> anyhow::Result<String> {
    if denom.eq_ignore_ascii_case(ETH_NATIVE_DENOM) {
        return Ok(ETH_NATIVE_DENOM.to_owned());
    }
    normalize_evm_address(denom)
}

pub fn token_address_to_denom(token: Address) -> String {
    if token == Address::ZERO {
        ETH_NATIVE_DENOM.to_owned()
    } else {
        evm_address_to_string(token)
    }
}

/// Build ACTION_EXECUTE payload for ETH transfers
pub fn encode_action_transfer_eth(recipient: &str, amount: u128) -> anyhow::Result<Vec<u8>> {
    let recipient = Address::from_str(recipient)?;
    Ok(encode_execute_action(recipient, U256::from(amount), vec![]))
}

/// Build ACTION_EXECUTE payload for ERC20(custom tokens) transfers
pub fn encode_action_transfer_erc20(
    token: &str,
    recipient: &str,
    amount: u128,
) -> anyhow::Result<Vec<u8>> {
    let token = Address::from_str(token)?;
    let recipient = Address::from_str(recipient)?;
    let transfer_call = IERC20::transferCall {
        recipient,
        amount: U256::from(amount),
    };
    Ok(encode_execute_action(
        token,
        U256::ZERO,
        transfer_call.abi_encode(),
    ))
}

pub fn encode_self_replace_action(
    validator_type: ValidatorType,
    current: PublicKey,
    replacement: PublicKey,
) -> Vec<u8> {
    let action = SelfReplaceAction {
        validatorType: match validator_type {
            ValidatorType::Listener => 0,
            ValidatorType::Processor => 1,
            ValidatorType::Approver => 2,
        },
        current: current.as_bytes().into_vec().into(),
        replacement: replacement.as_bytes().into_vec().into(),
    };
    abi_encode_u8_and_bytes(ACTION_SELF_REPLACE, &action.abi_encode())
}

fn signature_with_recovery_to_ethereum_bytes(
    signature: &SignatureWithRecovery,
) -> anyhow::Result<Vec<u8>> {
    let mut out = signature.sig.to_bytes();
    let recid = signature.recid.to_byte();
    anyhow::ensure!(
        recid <= 1,
        "Invalid Ethereum recovery id {recid}, expected 0 or 1"
    );
    out.push(recid + 27);
    Ok(out)
}

pub fn encode_new_set_action(
    validator_set: &ValidatorSet,
    rendered: &str,
    approvals: &[SignatureWithRecovery],
) -> anyhow::Result<Vec<u8>> {
    let approvals = approvals
        .iter()
        .map(signature_with_recovery_to_ethereum_bytes)
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .map(Into::into)
        .collect();
    let action = NewSetAction {
        processor: validator_set.processor.as_bytes().into_vec().into(),
        listeners: validator_set
            .listeners
            .iter()
            .copied()
            .map(|key| key.as_bytes().into_vec().into())
            .collect(),
        neededListeners: validator_set.needed_listeners,
        approvers: validator_set
            .approvers
            .iter()
            .copied()
            .map(|key| key.as_bytes().into_vec().into())
            .collect(),
        neededApprovers: validator_set.needed_approvers,
        rendered: rendered.as_bytes().to_vec().into(),
        approvals,
    };
    Ok(abi_encode_u8_and_bytes(
        ACTION_NEW_SET,
        &action.abi_encode(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        abi_encode_u64_and_bytes, abi_encode_u8_and_bytes, encode_action_transfer_erc20,
        encode_action_transfer_eth, normalize_ethereum_denom, normalize_evm_address,
        ACTION_EXECUTE, ETH_NATIVE_DENOM,
    };
    use alloy::primitives::{Address, U256};
    use std::str::FromStr;

    fn decode_execute_action_contract_abi(action: &[u8]) -> (Address, U256, Vec<u8>) {
        assert!(action.len() >= 128, "action payload too short");

        let target = Address::from_slice(&action[12..32]);
        let value = U256::from_be_slice(&action[32..64]);
        let data_offset = U256::from_be_slice(&action[64..96]).to::<usize>();
        assert_eq!(data_offset, 96, "unexpected execute calldata offset");

        let data_len = U256::from_be_slice(&action[96..128]).to::<usize>();
        let data_start = 128usize;
        let data_end = data_start + data_len;
        assert!(
            action.len() >= data_end,
            "execute calldata shorter than declared length"
        );

        (target, value, action[data_start..data_end].to_vec())
    }

    #[test]
    fn abi_encode_u64_and_bytes_matches_expected_layout() {
        let encoded = abi_encode_u64_and_bytes(1, b"abc");
        assert_eq!(encoded.len(), 128);

        // uint64 value in head[0]
        assert_eq!(&encoded[0..24], &[0u8; 24]);
        assert_eq!(&encoded[24..32], &1u64.to_be_bytes());

        // bytes offset in head[1]
        assert_eq!(&encoded[32..63], &[0u8; 31]);
        assert_eq!(encoded[63], 0x40);

        // bytes length in tail[0]
        assert_eq!(&encoded[64..88], &[0u8; 24]);
        assert_eq!(&encoded[88..96], &3u64.to_be_bytes());

        // bytes data + right-padding
        assert_eq!(&encoded[96..99], b"abc");
        assert!(encoded[99..128].iter().all(|x| *x == 0));
    }

    #[test]
    fn abi_encode_u8_and_bytes_matches_expected_layout() {
        let encoded = abi_encode_u8_and_bytes(7, b"abc");
        assert_eq!(encoded.len(), 128);

        // uint8 value in head[0]
        assert_eq!(&encoded[0..31], &[0u8; 31]);
        assert_eq!(encoded[31], 7);

        // bytes offset in head[1]
        assert_eq!(&encoded[32..63], &[0u8; 31]);
        assert_eq!(encoded[63], 0x40);

        // bytes length in tail[0]
        assert_eq!(&encoded[64..88], &[0u8; 24]);
        assert_eq!(&encoded[88..96], &3u64.to_be_bytes());

        // bytes data + right-padding
        assert_eq!(&encoded[96..99], b"abc");
        assert!(encoded[99..128].iter().all(|x| *x == 0));
    }

    #[test]
    fn encode_action_transfer_eth_uses_empty_calldata_and_amount_as_value() {
        let payload =
            encode_action_transfer_eth("0x1111111111111111111111111111111111111111", 42).unwrap();

        assert_eq!(payload[31], ACTION_EXECUTE);
        let action_bytes_len = u64::from_be_bytes(payload[88..96].try_into().unwrap()) as usize;
        let (target, value, data) =
            decode_execute_action_contract_abi(&payload[96..96 + action_bytes_len]);
        assert_eq!(
            target,
            Address::from_str("0x1111111111111111111111111111111111111111").unwrap()
        );
        assert_eq!(value, U256::from(42u128));
        assert!(data.is_empty());
    }

    #[test]
    fn encode_action_transfer_erc20_uses_transfer_calldata() {
        let payload = encode_action_transfer_erc20(
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            7,
        )
        .unwrap();

        assert_eq!(payload[31], ACTION_EXECUTE);
        let action_bytes_len = u64::from_be_bytes(payload[88..96].try_into().unwrap()) as usize;
        let (target, value, data) =
            decode_execute_action_contract_abi(&payload[96..96 + action_bytes_len]);
        assert_eq!(
            target,
            Address::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
        );
        assert_eq!(value, U256::from(0u128));
        assert_eq!(&data[0..4], &[0xa9, 0x05, 0x9c, 0xbb]); // transfer(address,uint256)
    }

    #[test]
    fn normalize_ethereum_denom_canonicalizes_eth_and_evm_addresses() {
        assert_eq!(normalize_ethereum_denom("ETH").unwrap(), ETH_NATIVE_DENOM);
        assert_eq!(
            normalize_ethereum_denom("0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap(),
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
    }

    #[test]
    fn normalize_evm_address_rejects_non_address() {
        assert!(normalize_evm_address("eth").is_err());
    }
}
