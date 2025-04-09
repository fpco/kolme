use std::collections::BTreeMap;

use shared::types::Sha256Hash;

use crate::*;

impl MerkleSerialize for u8 {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_byte(*self);
        Ok(())
    }
}

impl MerkleSerialize for u32 {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerialize for u64 {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerialize for usize {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_usize(*self);
        Ok(())
    }
}

impl MerkleSerialize for String {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes());
        Ok(())
    }
}

impl MerkleSerialize for Vec<u8> {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self);
        Ok(())
    }
}

impl MerkleSerialize for Sha256Hash {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(self.as_array());
        Ok(())
    }
}

impl<K: MerkleSerialize, V: MerkleSerialize> MerkleSerialize for BTreeMap<K, V> {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_usize(self.len());
        for (k, v) in self {
            k.serialize(serializer)?;
            v.serialize(serializer)?;
        }
        Ok(())
    }
}

impl MerkleSerialize for rust_decimal::Decimal {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_array(self.serialize());
        Ok(())
    }
}
