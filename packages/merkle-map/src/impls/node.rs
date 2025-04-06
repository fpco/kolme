use crate::*;

impl<K, V> Node<K, V> {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Node::Empty => true,
            Node::LockedLeaf(_)
            | Node::UnlockedLeaf(_)
            | Node::LockedTree(_)
            | Node::UnlockedTree(_) => false,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Node::Empty => 0,
            Node::LockedLeaf(leaf) => leaf.0.inner.len(),
            Node::UnlockedLeaf(leaf) => leaf.len(),
            Node::LockedTree(tree) => tree.0.inner.len(),
            Node::UnlockedTree(tree) => tree.len(),
        }
    }
}

impl<K: MerkleKey + Clone, V: Clone> Node<K, V> {
    pub(crate) fn unlock(self) -> UnlockedNode<K, V> {
        match self {
            Node::Empty => UnlockedNode::Leaf(LeafContents::default()),
            Node::LockedLeaf(leaf) => UnlockedNode::Leaf(leaf.into_inner()),
            Node::UnlockedLeaf(leaf) => UnlockedNode::Leaf(leaf),
            Node::LockedTree(tree) => UnlockedNode::Tree(tree.into_inner()),
            Node::UnlockedTree(tree) => UnlockedNode::Tree(*tree),
        }
    }

    pub(crate) fn get<Q>(&self, depth: u16, key_bytes: MerkleKeyBytes, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        match self {
            Node::Empty => None,
            Node::LockedLeaf(leaf) => leaf.0.inner.get(key_bytes, key),
            Node::UnlockedLeaf(leaf) => leaf.get(key_bytes, key),
            Node::LockedTree(tree) => tree.0.inner.get(depth, key_bytes, key),
            Node::UnlockedTree(tree) => tree.get(depth, key_bytes, key),
        }
    }
}
