use crate::*;

impl FromMerkleKey for String {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        std::str::from_utf8(bytes)
            .map(String::from)
            .map_err(MerkleSerialError::custom)
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
        bytes
            .try_into()
            .map(u32::from_le_bytes)
            .map_err(MerkleSerialError::custom)
    }
}
impl FromMerkleKey for u64 {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError> {
        bytes
            .try_into()
            .map(u64::from_le_bytes)
            .map_err(MerkleSerialError::custom)
    }
}
