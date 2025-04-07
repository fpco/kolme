use shared::types::Sha256Hash;

use crate::*;

impl MerkleDeserialize for u8 {
    fn deserialize<D: MerkleDeserializer>(deserializer: &mut D) -> Result<Self, MerkleSerialError> {
        deserializer.pop_byte()
    }
}

impl MerkleDeserialize for u32 {
    fn deserialize<D: MerkleDeserializer>(deserializer: &mut D) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u32::from_le_bytes)
    }
}

impl MerkleDeserialize for Sha256Hash {
    fn deserialize<D: MerkleDeserializer>(deserializer: &mut D) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(Sha256Hash::from_array)
    }
}
