mod key_bytes;

pub use key_bytes::MerkleBytes;

use std::sync::Arc;

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
    pub(crate) key_bytes: MerkleBytes,
    pub(crate) key: K,
    pub(crate) value: V,
}

pub(crate) struct Locked<T> {
    pub(crate) hash: Sha256Hash,
    pub(crate) payload: Arc<[u8]>,
    pub(crate) inner: Arc<T>,
}

impl<T: PartialOrd> PartialOrd for Locked<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.inner.partial_cmp(&other.inner)
    }
}

impl<T: Ord> Ord for Locked<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl<T: PartialEq> PartialEq for Locked<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq(&other.inner)
    }
}

impl<T: Eq> Eq for Locked<T> {}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
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
