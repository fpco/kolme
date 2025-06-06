use crate::*;

impl<K, V: MerkleSerializeRaw> MerkleSerializeRaw for LeafEntry<K, V> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.key_bytes.as_slice());
        self.value.merkle_serialize_raw(serializer)?;
        Ok(())
    }
}

impl<K: FromMerkleKey, V: MerkleDeserializeRaw> MerkleDeserializeRaw for LeafEntry<K, V> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let key_bytes = deserializer.load_bytes()?;
        let key = K::from_merkle_key(key_bytes)?;
        let key_bytes = MerkleKey::from_slice(key_bytes);
        let value = V::merkle_deserialize_raw(deserializer)?;
        Ok(LeafEntry {
            key_bytes,
            key,
            value,
        })
    }
}
