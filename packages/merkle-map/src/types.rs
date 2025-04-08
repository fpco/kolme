mod key_bytes;

pub use key_bytes::MerkleKey;

pub(crate) use crate::impls::Lockable;

use std::sync::{Arc, OnceLock};

use shared::types::Sha256Hash;

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

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Node<K, V> {
    #[default]
    Empty,
    Leaf(Lockable<LeafContents<K, V>>),
    Tree(Lockable<TreeContents<K, V>>),
    // FIXME
    // Lazy {
    //     hash: Sha256Hash,
    // },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LeafContents<K, V> {
    /// Invariant: must be sored by key_bytes
    pub(crate) values: Vec<LeafEntry<K, V>>, // FIXME switch to an array
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TreeContents<K, V> {
    pub(crate) len: usize,
    pub(crate) leaf: Option<LeafEntry<K, V>>,
    pub(crate) branches: [Node<K, V>; 16],
}
