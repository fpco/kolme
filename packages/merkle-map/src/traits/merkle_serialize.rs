use shared::types::Sha256Hash;

use crate::*;

impl MerkleSerialize for u8 {
    fn serialize<S: MerkleSerializer>(
        &mut self,
        serializer: &mut S,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_byte(*self);
        Ok(())
    }
}

impl MerkleSerialize for u32 {
    fn serialize<S: MerkleSerializer>(
        &mut self,
        serializer: &mut S,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerialize for String {
    fn serialize<S: MerkleSerializer>(
        &mut self,
        serializer: &mut S,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes());
        Ok(())
    }
}

impl MerkleSerialize for Vec<u8> {
    fn serialize<S: MerkleSerializer>(
        &mut self,
        serializer: &mut S,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self);
        Ok(())
    }
}

impl MerkleSerialize for Sha256Hash {
    fn serialize<S: MerkleSerializer>(
        &mut self,
        serializer: &mut S,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(self.as_array());
        Ok(())
    }
}
