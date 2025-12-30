use shared::{
    cryptography::PublicKey,
    types::{BridgeActionId, BridgeEventId},
};

use crate::*;

impl FromMerkleKey for String {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        Ok(std::str::from_utf8(bytes)?.to_string())
    }
}

impl FromMerkleKey for u8 {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        if bytes.len() == 1 {
            Ok(bytes[0])
        } else {
            Err(MerkleSerialError::InsufficientInput)
        }
    }
}

impl FromMerkleKey for u32 {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        let arr: [u8; 4] =
            bytes
                .try_into()
                .map_err(|_| MerkleSerialError::InvalidMerkleKeyLength {
                    expected: 4,
                    actual: bytes.len(),
                })?;
        Ok(u32::from_be_bytes(arr))
    }
}

impl FromMerkleKey for u64 {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        let arr: [u8; 8] =
            bytes
                .try_into()
                .map_err(|_| MerkleSerialError::InvalidMerkleKeyLength {
                    expected: 8,
                    actual: bytes.len(),
                })?;
        Ok(u64::from_be_bytes(arr))
    }
}

impl FromMerkleKey for PublicKey {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        PublicKey::from_bytes(bytes).map_err(MerkleSerialError::custom)
    }
}

impl FromMerkleKey for BridgeEventId {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        u64::from_merkle_key(bytes).map(Self)
    }
}

impl FromMerkleKey for BridgeActionId {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        u64::from_merkle_key(bytes).map(Self)
    }
}
