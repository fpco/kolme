mod from_merkle_key;
mod merkle_deserialize;
mod merkle_serialize;
mod merkle_serialize_complete;
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
    /// Serialize this data for storage.
    fn serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError>;
}

/// A value that can be serialized into its own top level hash.
pub trait MerkleSerializeComplete {
    fn serialize_complete(
        &self,
        manager: &mut MerkleSerializeManager,
    ) -> Result<Sha256Hash, MerkleSerialError>;
}

/// A value that can be deserialized back into a [MerkleMap] value.
pub trait MerkleDeserialize: Sized {
    fn deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError>;
}

/// A backing store for raw blobs used by a [MerkleMap].
pub trait MerkleStore {
    /// Load up the blob by hash, if available.
    #[allow(async_fn_in_trait)]
    async fn load_merkle_by_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<Arc<[u8]>>, MerkleSerialError>;

    /// Save the payload within the Merkle store.
    ///
    /// Invariant: the hash must be the correct hash of the given payload.
    #[allow(async_fn_in_trait)]
    async fn save_merkle_by_hash(
        &mut self,
        hash: Sha256Hash,
        payload: &[u8],
    ) -> Result<(), MerkleSerialError>;

    /// Checks if the store already has a blob matching the given hash.
    #[allow(async_fn_in_trait)]
    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError>;
}

pub(crate) trait CanLock {
    fn lock(&self) -> Result<(Sha256Hash, Arc<[u8]>), MerkleSerialError>;
}
