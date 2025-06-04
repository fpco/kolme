use std::fmt::Debug;

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
        Node::Leaf(MerkleLockable::new(LeafContents::default()))
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

impl<K, V> Node<K, V> {
    pub(crate) fn get(&self, depth: u16, key_bytes: &MerkleKey) -> Option<&LeafEntry<K, V>> {
        match self {
            Node::Leaf(leaf) => leaf.as_ref().get(key_bytes),
            Node::Tree(tree) => tree.as_ref().get(depth, key_bytes),
        }
    }
}

impl<K: Clone, V: Clone> Node<K, V> {
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

impl<K: FromMerkleKey, V: MerkleSerialize> Node<K, V> {
    pub(crate) fn hash(&self) -> Sha256Hash {
        match self {
            Node::Leaf(leaf) => leaf.as_ref().hash(),
            Node::Tree(tree) => tree.as_ref().hash(),
        }
    }
}

impl<K: ToMerkleKey, V: MerkleSerialize> MerkleSerialize for Node<K, V> {
    fn get_merkle_contents(&self) -> Option<Arc<MerkleContents>> {
        match self {
            Node::Leaf(leaf) => leaf.get_merkle_contents(),
            Node::Tree(tree) => tree.get_merkle_contents(),
        }
    }

    fn set_merkle_contents(&self, contents: &Arc<MerkleContents>) {
        match self {
            Node::Leaf(leaf) => leaf.set_merkle_contents(contents),
            Node::Tree(tree) => tree.set_merkle_contents(contents),
        }
    }

    fn merkle_serialize(&self, manager: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        match self {
            Node::Leaf(leaf) => leaf.merkle_serialize(manager),
            Node::Tree(tree) => tree.merkle_serialize(manager),
        }
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> MerkleDeserialize for Node<K, V> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        match deserializer.peek_byte()? {
            42 => MerkleLockable::<LeafContents<K, V>>::merkle_deserialize(deserializer)
                .map(Node::Leaf),
            43 => MerkleLockable::<TreeContents<K, V>>::merkle_deserialize(deserializer)
                .map(Node::Tree),
            byte => Err(MerkleSerialError::UnexpectedMagicByte { byte }),
        }
    }
}

impl<K: Debug, V: Debug> Debug for Node<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{{")?;
        for (idx, (k, v)) in self.iter().enumerate() {
            if idx != 0 {
                write!(f, ", ")?;
            }
            write!(f, "{k:?}: {v:?}")?;
        }
        write!(f, "}}")?;
        Ok(())
    }
}
