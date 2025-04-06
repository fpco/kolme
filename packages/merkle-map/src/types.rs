use std::sync::Arc;

use shared::types::Sha256Hash;

pub struct MerkleTree<K, V>(pub(crate) Node<K, V>);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct MerkleKeyBytes(pub(crate) Vec<u8>);

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
    pub(crate) branches: [Node<K, V>; 16],
}

pub struct Iter<'a, K, V> {
    pub(crate) tree: &'a MerkleTree<K, V>,
    pub(crate) cursor: Cursor,
}

#[derive(Default)]
pub(crate) struct Cursor(pub(crate) Vec<u8>);

pub struct IntoIter<K, V>(pub(crate) MerkleTree<K, V>);
