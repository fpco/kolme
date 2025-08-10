use crate::*;

impl<T: MerkleSerialize> MerkleSerializeRaw for T {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&T::merkle_version())?;
        T::merkle_serialize(self, serializer)
    }

    fn get_merkle_hash_raw(&self) -> Option<Sha256Hash> {
        self.get_merkle_hash()
    }

    fn set_merkle_hash_raw(&self, hash: Sha256Hash) {
        self.set_merkle_hash(hash);
    }
}

impl<T: MerkleSerialize + MerkleDeserialize> MerkleDeserializeRaw for T {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let version = deserializer.load()?;
        // TODO consider including some string identifying the data type in this error
        let highest_supported = Self::merkle_version();
        if version > highest_supported {
            Err(MerkleSerialError::UnexpectedVersion {
                highest_supported,
                actual: version,
                type_name: std::any::type_name::<T>(),
                offset: deserializer.get_position(),
            })
        } else {
            T::merkle_deserialize(deserializer, version)
        }
    }
}
