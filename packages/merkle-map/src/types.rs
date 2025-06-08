mod key_bytes;
#[cfg(test)]
pub mod quickcheck_newtypes;

use std::collections::HashSet;

use crate::*;
pub use key_bytes::MerkleKey;
use smallvec::SmallVec;

pub(crate) use crate::impls::MerkleLockable;

/// A base16 tree supporting sharing of subtrees and cheap hashing.
///
/// Consider this as a replacement for a `BTreeMap`. Importantly,
/// this type internally will hash each subtree individually and store
/// its serialized format in separate database entries. The goal is to
/// allow for aggressive sharing in both the on-disk and in-memory
/// representation of the data.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct MerkleMap<K, V>(pub(crate) Node<K, V>);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LeafEntry<K, V> {
    pub(crate) key_bytes: MerkleKey,
    pub(crate) key: K,
    pub(crate) value: V,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Node<K, V> {
    Leaf(MerkleLockable<LeafContents<K, V>>),
    Tree(MerkleLockable<TreeContents<K, V>>),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LeafContents<K, V> {
    /// Invariant: must be sored by key_bytes
    pub(crate) values: arrayvec::ArrayVec<LeafEntry<K, V>, 16>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TreeContents<K, V> {
    pub(crate) len: usize,
    pub(crate) leaf: Option<LeafEntry<K, V>>,
    pub(crate) branches: [Node<K, V>; 16],
}

/// The serialized contents of a value.
///
/// This includes the hash and payload which will be stored in the database.
/// It also includes all direct children nodes encountered during serialization,
/// so that they can be checked as present in the database and added if missing.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct MerkleContents {
    pub hash: Sha256Hash,
    #[serde(
        serialize_with = "serialize_base64",
        deserialize_with = "deserialize_base64"
    )]
    pub payload: Arc<[u8]>,
    pub children: Arc<[Arc<MerkleContents>]>,
}

/// The contents of a single layer of a merkle data structure.
///
/// This is an intermediate data structure used for interacting with storage.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MerkleLayerContents {
    /// The contents of this layer.
    pub payload: Arc<[u8]>,
    /// The hashes of the direct children of this layer.
    pub children: SmallVec<[Sha256Hash; 16]>,
}

fn serialize_base64<S>(data: &Arc<[u8]>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use base64::prelude::*;
    let base64 = BASE64_STANDARD.encode(data);
    serializer.serialize_str(&base64)
}

// Deserialize base64 string to Arc<[u8]>
fn deserialize_base64<'de, D>(deserializer: D) -> Result<Arc<[u8]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use base64::prelude::*;
    let base64 = <String as serde::Deserialize>::deserialize(deserializer)?;
    let bytes = BASE64_STANDARD
        .decode(&base64)
        .map_err(serde::de::Error::custom)?;
    Ok(Arc::from(bytes))
}

/// Errors that can occur during serialization of data.
#[derive(thiserror::Error, Debug)]
pub enum MerkleSerialError {
    #[error("Insufficient input when parsing buffer")]
    InsufficientInput,
    #[error("A usize value would be larger than the machine representation")]
    UsizeOverflow,
    #[error(
        "Unexpected magic byte to distinguish Tree from Leaf, expected 0 or 1, but got {byte}"
    )]
    UnexpectedMagicByte { byte: u8 },
    #[error("Invalid byte at start of tree, expected 0 or 1, but got {byte}")]
    InvalidTreeStart { byte: u8 },
    #[error("Leftover input was unconsumed")]
    TooMuchInput,
    #[error("Serialized content was invalid")]
    InvalidSerializedContent,
    #[error("Hashes not found in store: {hashes:?}")]
    HashesNotFound {
        hashes: HashSet<shared::types::Sha256Hash>,
    },
    #[error("Leaf content limit exceeded: limit {limit}, actual {actual}")]
    LeafContentLimitExceeded { limit: usize, actual: usize },
    #[error(transparent)]
    Custom(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    Other(String),
}
