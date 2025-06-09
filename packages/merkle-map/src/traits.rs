mod from_merkle_key;
mod merkle_deserialize;
mod merkle_serialize;
#[cfg(test)]
pub mod quickcheck_arbitrary;
mod to_merkle_key;

use std::sync::Arc;

use shared::types::Sha256Hash;

use crate::*;

/// Values which can be used as keys in a [crate::MerkleMap].
pub trait ToMerkleKey {
    fn to_merkle_key(&self) -> MerkleKey;
}

/// Values which can be parsed back from a rendered merkle key.
///
/// This trait is kept separate from [ToMerkleKey] to allow
/// rendering keys from non-[Sized] types.
pub trait FromMerkleKey: Sized {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError>;
}

/// A value that can be serialized within a [MerkleMap].
pub trait MerkleSerialize {
    const MERKLE_SERIALIZE_VERSION: Option<usize> = None;

    /// Serialize this data for storage.
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError>;

    fn merkle_serialize_with_version(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        if let Some(version) = Self::MERKLE_SERIALIZE_VERSION {
            serializer.store(&version)?;
        };
        self.merkle_serialize(serializer)
    }

    /// Optimization: if we already know our serialized contents, return them.
    fn get_merkle_contents(&self) -> Option<Arc<MerkleContents>> {
        None
    }

    /// Update the cached Merkle hash and payload, if supported by this type.
    fn set_merkle_contents(&self, _contents: &Arc<MerkleContents>) {}
}

/// A value that can be deserialized back into a [MerkleMap] value.
pub trait MerkleDeserialize: Sized {
    const MERKLE_DESERIALIZE_VERSION: Option<usize> = None;

    fn merkle_deserialize(deserializer: &mut MerkleDeserializer)
        -> Result<Self, MerkleSerialError>;

    fn merkle_deserialize_with_version(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        if let Some(code_version) = Self::MERKLE_DESERIALIZE_VERSION {
            let data_version: usize = deserializer.load()?;
            match data_version.cmp(&code_version) {
                std::cmp::Ordering::Less => {
                    Self::deserialize_previous_version(deserializer, data_version)
                }
                std::cmp::Ordering::Equal => Self::merkle_deserialize(deserializer),
                std::cmp::Ordering::Greater => Err(MerkleSerialError::UnexpectedVersion {
                    highest_supported: code_version,
                    actual: data_version,
                    type_name: std::any::type_name::<Self>(),
                    offset: deserializer.get_position(),
                }),
            }
        } else {
            Self::merkle_deserialize(deserializer)
        }
    }

    fn deserialize_previous_version(
        _deserializer: &mut MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError> {
        unimplemented!(
            "Deserializing version {version} of {} is not implemented!",
            std::any::type_name::<Self>()
        )
    }

    fn set_merkle_contents(&self, _contents: Arc<MerkleContents>) {}
}

/// A backing store for raw blobs used by a [MerkleMap].
pub trait MerkleStore {
    /// Load up the blob by hash, if available.
    ///
    /// This loads up a single layer of a merkle hash, by returning
    /// both the raw payload for this level, plus the hashes of
    /// any children. This allows the library to reconstruct
    /// the entire [MerkleContents] for any given hash.
    #[allow(async_fn_in_trait)]
    async fn load_by_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleLayerContents>, MerkleSerialError>;

    /// Save the payload within the Merkle store.
    ///
    /// Invariant: the hash must be the correct hash of the given payload.
    #[allow(async_fn_in_trait)]
    async fn save_by_hash(
        &mut self,
        hash: Sha256Hash,
        layer: &MerkleLayerContents,
    ) -> Result<(), MerkleSerialError>;

    /// Checks if the store already has a blob matching the given hash.
    #[allow(async_fn_in_trait)]
    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError>;
}
