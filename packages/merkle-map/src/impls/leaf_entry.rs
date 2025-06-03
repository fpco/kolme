use sha2::{Digest, Sha256};

use crate::*;

impl<K, V: MerkleSerialize> MerkleSerialize for LeafEntry<K, V> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.key_bytes.as_slice());
        self.value.merkle_serialize(serializer)?;
        Ok(())
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> MerkleDeserialize for LeafEntry<K, V> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let key_bytes = deserializer.load_bytes()?;
        let key = K::from_merkle_key(key_bytes)?;
        let key_bytes = MerkleKey::from_slice(key_bytes);
        let value = V::merkle_deserialize(deserializer)?;
        Ok(LeafEntry {
            key_bytes,
            key,
            value,
        })
    }
}

impl<K: AsRef<[u8]> + FromMerkleKey + MerkleSerialize, V: AsRef<[u8]> + MerkleSerialize> LeafEntry<K, V> {
    pub(crate) fn hash(&self) -> Sha256Hash {
        let mut hasher = Sha256::new();
        hasher.update(self.key_bytes.as_slice());
        let mut serializer = MerkleSerializer::new(MerkleManager::default());
        self.value.merkle_serialize(&mut serializer).expect("Serialization failed");
        Sha256Hash::from_array(hasher.finalize().into())
    }
}