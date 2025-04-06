mod key_bytes;

pub use key_bytes::MerkleKeyBytes;

use std::sync::Arc;

use shared::types::Sha256Hash;

/// A base16 tree supporting sharing of subtrees and cheap hashing.
///
/// Consider this as a replacement for a `BTreeMap`. Importantly,
/// this type internally will hash each subtree individually and store
/// its serialized format in separate database entries. The goal is to
/// allow for aggressive sharing in both the on-disk and in-memory
/// representation of the data.
pub struct MerkleMap<K, V>(pub(crate) Node<K, V>);

#[derive(Clone)]
pub(crate) struct LeafEntry<K, V> {
    pub(crate) key_bytes: MerkleKeyBytes,
    pub(crate) key: K,
    pub(crate) value: V,
}

pub(crate) struct Locked<T>(pub(crate) Arc<LockedInner<T>>);

pub(crate) struct LockedInner<T> {
    pub(crate) hash: Sha256Hash,
    pub(crate) inner: T,
}

#[derive(Clone, Default)]
pub(crate) enum Node<K, V> {
    #[default]
    Empty,
    LockedLeaf(Locked<LeafContents<K, V>>),
    UnlockedLeaf(LeafContents<K, V>),
    LockedTree(Locked<TreeContents<K, V>>),
    UnlockedTree(Box<TreeContents<K, V>>),
    // FIXME
    // Lazy {
    //     hash: Sha256Hash,
    // },
}

pub(crate) enum UnlockedNode<K, V> {
    Leaf(LeafContents<K, V>),
    Tree(TreeContents<K, V>),
}

#[derive(Clone)]
pub(crate) struct LeafContents<K, V> {
    /// Invariant: must be sored by key_bytes
    pub(crate) values: Vec<LeafEntry<K, V>>, // FIXME switch to an array
}

#[derive(Clone)]
pub(crate) struct TreeContents<K, V> {
    pub(crate) len: usize,
    pub(crate) leaf: Option<LeafEntry<K, V>>,
    pub(crate) branches: [Node<K, V>; 16],
}
