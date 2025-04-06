use crate::*;

impl<K: MerkleKey + Clone, V: Clone> From<UnlockedNode<K, V>> for Node<K, V> {
    fn from(node: UnlockedNode<K, V>) -> Self {
        match node {
            UnlockedNode::Leaf(leaf) => {
                if leaf.is_empty() {
                    Node::Empty
                } else {
                    Node::UnlockedLeaf(leaf)
                }
            }
            UnlockedNode::Tree(tree) => {
                let count = tree.len();
                if count == 0 {
                    Node::Empty
                } else if count <= 16 {
                    Node::UnlockedLeaf(tree.into())
                } else {
                    Node::UnlockedTree(Box::new(tree))
                }
            }
        }
    }
}
impl<K: MerkleKey + Clone, V: Clone> UnlockedNode<K, V> {
    pub(crate) fn insert(
        self,
        depth: u16,
        entry: LeafEntry<K, V>,
    ) -> (UnlockedNode<K, V>, Option<(K, V)>) {
        match self {
            UnlockedNode::Leaf(leaf) => leaf.insert(depth, entry),
            UnlockedNode::Tree(mut tree) => {
                let v = tree.insert(depth, entry);
                (UnlockedNode::Tree(tree), v)
            }
        }
    }

    pub(crate) fn remove<Q>(
        self,
        depth: u16,
        key_bytes: MerkleKeyBytes,
        key: &Q,
    ) -> (UnlockedNode<K, V>, Option<(K, V)>)
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        match self {
            UnlockedNode::Leaf(mut leaf) => {
                let v = leaf.remove(key_bytes, key);
                (UnlockedNode::Leaf(leaf), v)
            }
            UnlockedNode::Tree(tree) => tree.remove(depth, key_bytes, key),
        }
    }
}
