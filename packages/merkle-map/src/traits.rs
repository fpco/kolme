mod from_merkle_key;
mod merkle_deserialize;
mod merkle_serialize;
#[cfg(test)]
pub mod quickcheck_arbitrary;
mod to_merkle_key;

use std::{collections::HashMap, sync::Arc};

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
///
/// Types that implement this trait will always be serialized and deserialized
/// with a version number. If you have a data type with a fixed format (e.g. primitive
/// types like `u32`), you can use [MerkleSerializeRaw] instead.
pub trait MerkleSerialize {
    /// Serialize this data for storage.
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError>;

    /// The version number of the serialized format.
    ///
    /// Defaults to `0` for convenience.
    fn merkle_version() -> usize {
        0
    }

    fn get_merkle_contents(&self) -> Option<Arc<MerkleContents>> {
        None
    }

    fn set_merkle_contents(&self, _contents: Arc<MerkleContents>) {}
}

/// A type that can be serialized, without requiring a version number in its payload.
///
/// Note that all types that implement [MerkleSerialize] also get an impl for this trait.
pub trait MerkleSerializeRaw {
    /// Serialize this data for storage.
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError>;

    fn get_merkle_contents_raw(&self) -> Option<Arc<MerkleContents>> {
        None
    }

    fn set_merkle_contents_raw(&self, _contents: Arc<MerkleContents>) {}
}

/// A value that can be deserialized back into a [MerkleMap] value.
///
/// See description of [MerkleSerialize] for information on serialization of
/// versions.
pub trait MerkleDeserialize: Sized {
    /// Deserialize from stored data.
    ///
    /// Note that, when implementing this method, you can rely on an invariant
    /// that the version number will never be greater than [MerkleSerialize::merkle_version].
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        version: usize,
    ) -> Result<Self, MerkleSerialError>;

    fn set_merkle_contents(&self, _contents: &Arc<MerkleContents>) {}
}

pub trait MerkleDeserializeRaw: Sized {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError>;

    fn load_merkle_by_hash(_hash: Sha256Hash) -> Option<Self> {
        None
    }
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
    async fn load_by_hashes(
        &mut self,
        hashes: &[Sha256Hash],
        dest: &mut HashMap<Sha256Hash, MerkleLayerContents>,
    ) -> Result<(), MerkleSerialError>;

    /// Save the payload within the Merkle store.
    ///
    /// Invariant: the hash must be the correct hash of the given payload.
    #[allow(async_fn_in_trait)]
    async fn save_by_hash(&mut self, layer: &MerkleLayerContents) -> Result<(), MerkleSerialError>;

    /// Checks if the store already has a blob matching the given hash.
    #[allow(async_fn_in_trait)]
    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError>;
}
