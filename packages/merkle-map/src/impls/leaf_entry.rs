use crate::*;

impl<K, V: MerkleSerialize> MerkleSerialize for LeafEntry<K, V> {
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.key_bytes.as_slice());
        self.value.serialize(serializer)?;
        Ok(())
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> MerkleDeserialize for LeafEntry<K, V> {
    fn deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        let key_bytes = deserializer.load_bytes()?;
        let key = K::from_merkle_key(key_bytes)?;
        let key_bytes = MerkleKey::from_slice(key_bytes);
        let value = V::deserialize(deserializer)?;
        Ok(LeafEntry {
            key_bytes,
            key,
            value,
        })
    }
}
