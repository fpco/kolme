use std::collections::BTreeMap;

use shared::types::Sha256Hash;

use crate::*;

impl MerkleDeserialize for u8 {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        deserializer.pop_byte()
    }
}

impl MerkleDeserialize for u32 {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u32::from_le_bytes)
    }
}

impl MerkleDeserialize for u64 {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u64::from_le_bytes)
    }
}

impl MerkleDeserialize for usize {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        deserializer.load_usize()
    }
}

impl MerkleDeserialize for String {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        let bytes = deserializer.load_bytes()?;
        std::str::from_utf8(bytes)
            .map(ToOwned::to_owned)
            .map_err(MerkleSerialError::custom)
    }
}

impl MerkleDeserialize for Sha256Hash {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(Sha256Hash::from_array)
    }
}

impl<K: MerkleDeserialize + Ord, V: MerkleDeserialize> MerkleDeserialize for BTreeMap<K, V> {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        let len = deserializer.load_usize()?;
        let mut x = BTreeMap::new();
        for _ in 0..len {
            let k = K::merkle_deserialize(deserializer)?;
            let v = V::merkle_deserialize(deserializer)?;
            x.insert(k, v);
        }
        Ok(x)
    }
}

impl MerkleDeserialize for rust_decimal::Decimal {
    fn merkle_deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        deserializer
            .load_array()
            .map(rust_decimal::Decimal::deserialize)
    }
}
