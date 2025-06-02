use kolme::{MerkleDeserialize, MerkleSerialize};

use crate::VersionUpgradeTestState;

impl MerkleSerialize for VersionUpgradeTestState {
    fn merkle_serialize(
        &self,
        _serializer: &mut kolme::MerkleSerializer,
    ) -> Result<(), kolme::MerkleSerialError> {
        Ok(())
    }
}

impl MerkleDeserialize for VersionUpgradeTestState {
    fn merkle_deserialize(
        _deserializer: &mut kolme::MerkleDeserializer,
    ) -> Result<Self, kolme::MerkleSerialError> {
        Ok(Self {})
    }
}
