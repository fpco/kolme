use crate::*;

impl<K, V: ToMerkleBytes> LeafEntry<K, V> {
    pub(crate) fn store(&self, buff: &mut WriteBuffer) {
        buff.store_slice(self.key_bytes.as_slice());
        buff.store_slice(self.value.to_merkle_bytes().as_slice());
    }
}

impl<K: FromMerkleBytes, V: FromMerkleBytes> LeafEntry<K, V> {
    pub(crate) fn load<Store: MerkleRead>(
        buff: &mut ReadBuffer,
    ) -> Result<Self, LoadMerkleMapError<Store::Error>> {
        let key_bytes = buff.load_bytes()?;
        let key =
            K::from_merkle_bytes(key_bytes).ok_or(LoadMerkleMapError::InvalidSerializedContent)?;
        let key_bytes = MerkleBytes::from_slice(key_bytes);
        let value_bytes = buff.load_bytes()?;
        let value = V::from_merkle_bytes(&value_bytes)
            .ok_or(LoadMerkleMapError::InvalidSerializedContent)?;
        Ok(LeafEntry {
            key_bytes,
            key,
            value,
        })
    }
}
