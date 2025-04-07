use crate::*;

impl<K: Clone, V: Clone> From<UnlockedNode<K, V>> for Node<K, V> {
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
impl<K: Clone, V: Clone> UnlockedNode<K, V> {
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

    pub(crate) fn remove(
        self,
        depth: u16,
        key_bytes: MerkleKey,
    ) -> (UnlockedNode<K, V>, Option<(K, V)>) {
        match self {
            UnlockedNode::Leaf(mut leaf) => {
                let v = leaf.remove(key_bytes);
                (UnlockedNode::Leaf(leaf), v)
            }
            UnlockedNode::Tree(tree) => tree.remove(depth, key_bytes),
        }
    }
}
