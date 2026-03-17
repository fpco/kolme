//! Ethereum-specific helpers.
#![cfg(feature = "ethereum")]

use std::str::FromStr;

use alloy::{
    primitives::{Address, U256},
    sol,
    sol_types::SolValue,
};

use crate::SignatureWithRecovery;
use crate::{AssetAmount, PublicKey, ValidatorSet, ValidatorType};

pub const ACTION_TRANSFER_ETH: u8 = 0;
pub const ACTION_TRANSFER_ERC20: u8 = 1;
pub const ACTION_SELF_REPLACE: u8 = 2;
pub const ACTION_NEW_SET: u8 = 3;
pub const ACTION_EXECUTE: u8 = 4;

sol! {
    struct TransferEthAction {
        address recipient;
        uint256 amount;
    }

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
}

/// ABI-encode execute_signed() payload `(uint64 action_id, bytes action_data)` according
/// to Solidity ABI rules.
pub fn abi_encode_u64_and_bytes(value: u64, data: &[u8]) -> Vec<u8> {
    const WORD_SIZE: usize = 32;

    let mut encoded =
        Vec::with_capacity(WORD_SIZE * 3 + data.len().div_ceil(WORD_SIZE) * WORD_SIZE);

    // head[0] = uint64
    let mut word = [0u8; WORD_SIZE];
    word[WORD_SIZE - 8..].copy_from_slice(&value.to_be_bytes());
    encoded.extend_from_slice(&word);

    // head[1] = offset to dynamic bytes argument
    word.fill(0);
    word[WORD_SIZE - 1] = 0x40;
    encoded.extend_from_slice(&word);

    // tail[0] = bytes length
    word.fill(0);
    word[WORD_SIZE - 8..].copy_from_slice(&(data.len() as u64).to_be_bytes());
    encoded.extend_from_slice(&word);

    // tail[1..] = bytes data + right padding to 32-byte boundary
    encoded.extend_from_slice(data);
    let padding = (WORD_SIZE - (data.len() % WORD_SIZE)) % WORD_SIZE;
    if padding > 0 {
        encoded.resize(encoded.len() + padding, 0);
    }

    encoded
}

/// ABI-encode _executeAction() payload `(uint8, bytes)` in bridge contract
pub fn abi_encode_u8_and_bytes(value: u8, data: &[u8]) -> Vec<u8> {
    const WORD_SIZE: usize = 32;

    let mut encoded =
        Vec::with_capacity(WORD_SIZE * 3 + data.len().div_ceil(WORD_SIZE) * WORD_SIZE);

    // head[0] = uint8
    let mut word = [0u8; WORD_SIZE];
    word[WORD_SIZE - 1] = value;
    encoded.extend_from_slice(&word);

    // head[1] = offset to dynamic bytes argument
    word.fill(0);
    word[WORD_SIZE - 1] = 0x40;
    encoded.extend_from_slice(&word);

    // tail[0] = bytes length
    word.fill(0);
    word[WORD_SIZE - 8..].copy_from_slice(&(data.len() as u64).to_be_bytes());
    encoded.extend_from_slice(&word);

    // tail[1..] = bytes data + right padding to 32-byte boundary
    encoded.extend_from_slice(data);
    let padding = (WORD_SIZE - (data.len() % WORD_SIZE)) % WORD_SIZE;
    if padding > 0 {
        encoded.resize(encoded.len() + padding, 0);
    }

    encoded
}

pub fn encode_transfer_eth_action(
    recipient: &str,
    funds: &[AssetAmount],
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        funds.len() == 1,
        "Ethereum transfer action supports exactly one fund, got {}",
        funds.len()
    );
    let fund = funds[0].clone();
    let recipient = Address::from_str(recipient)?;
    let transfer = TransferEthAction {
        recipient,
        amount: U256::from(fund.amount),
    };
    let action = ExecuteAction {
        target: transfer.recipient,
        value: transfer.amount,
        data: vec![].into(),
    };
    Ok(abi_encode_u8_and_bytes(ACTION_EXECUTE, &action.abi_encode()))
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
    use super::{abi_encode_u64_and_bytes, abi_encode_u8_and_bytes};

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
}
