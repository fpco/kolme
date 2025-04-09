use crate::*;

impl<K, V> Clone for Node<K, V> {
    fn clone(&self) -> Self {
        match self {
            Node::Leaf(leaf) => Node::Leaf(leaf.clone()),
            Node::Tree(tree) => Node::Tree(tree.clone()),
        }
    }
}

impl<K, V> Default for Node<K, V> {
    fn default() -> Self {
        Node::Leaf(Lockable::new_unlocked(LeafContents::default()))
    }
}

impl<K, V> Node<K, V> {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Node::Leaf(leaf) => leaf.as_ref().is_empty(),
            Node::Tree(tree) => {
                debug_assert_ne!(tree.as_ref().len(), 0);
                false
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Node::Leaf(leaf) => leaf.as_ref().len(),
            Node::Tree(tree) => tree.as_ref().len(),
        }
    }
}

impl<K: Clone, V: Clone> Node<K, V> {
    pub(crate) fn get(&self, depth: u16, key_bytes: &MerkleKey) -> Option<&V> {
        match self {
            Node::Leaf(leaf) => leaf.as_ref().get(key_bytes),
            Node::Tree(tree) => tree.as_ref().get(depth, key_bytes),
        }
    }

    pub(crate) fn get_mut(&mut self, depth: u16, key_bytes: &MerkleKey) -> Option<&mut V> {
        match self {
            Node::Leaf(leaf) => leaf.as_mut().get_mut(key_bytes),
            Node::Tree(tree) => tree.as_mut().get_mut(depth, key_bytes),
        }
    }
}

impl<K: Clone, V: Clone> Node<K, V> {
    pub(crate) fn insert(self, depth: u16, entry: LeafEntry<K, V>) -> (Node<K, V>, Option<(K, V)>) {
        match self {
            Node::Leaf(leaf) => leaf.into_inner().insert(depth, entry),
            Node::Tree(mut tree) => {
                let v = tree.as_mut().insert(depth, entry);
                (Node::Tree(tree), v)
            }
        }
    }

    pub(crate) fn remove(self, depth: u16, key_bytes: MerkleKey) -> (Node<K, V>, Option<(K, V)>) {
        match self {
            Node::Leaf(leaf) => leaf.into_inner().remove(key_bytes),
            Node::Tree(tree) => tree.into_inner().remove(depth, key_bytes),
        }
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> MerkleDeserialize for Node<K, V> {
    fn deserialize(deserializer: &mut MerkleDeserializer) -> Result<Self, MerkleSerialError> {
        match deserializer.pop_byte()? {
            42 => Lockable::<LeafContents<K, V>>::deserialize(deserializer).map(Node::Leaf),
            43 => Lockable::<TreeContents<K, V>>::deserialize(deserializer).map(Node::Tree),
            byte => Err(MerkleSerialError::UnexpectedMagicByte { byte }),
        }
    }
}
