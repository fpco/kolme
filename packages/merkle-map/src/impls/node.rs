use shared::types::Sha256Hash;

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
            Node::LockedLeaf(leaf) => leaf.inner.len(),
            Node::UnlockedLeaf(leaf) => leaf.len(),
            Node::LockedTree(tree) => tree.inner.len(),
            Node::UnlockedTree(tree) => tree.len(),
        }
    }
}

impl<K: ToMerkleBytes + Clone, V: Clone> Node<K, V> {
    pub(crate) fn unlock(self) -> UnlockedNode<K, V> {
        match self {
            Node::Empty => UnlockedNode::Leaf(LeafContents::default()),
            Node::LockedLeaf(leaf) => UnlockedNode::Leaf(leaf.into_inner()),
            Node::UnlockedLeaf(leaf) => UnlockedNode::Leaf(leaf),
            Node::LockedTree(tree) => UnlockedNode::Tree(tree.into_inner()),
            Node::UnlockedTree(tree) => UnlockedNode::Tree(*tree),
        }
    }

    pub(crate) fn get(&self, depth: u16, key_bytes: MerkleBytes) -> Option<&V> {
        match self {
            Node::Empty => None,
            Node::LockedLeaf(leaf) => leaf.inner.get(key_bytes),
            Node::UnlockedLeaf(leaf) => leaf.get(key_bytes),
            Node::LockedTree(tree) => tree.inner.get(depth, key_bytes),
            Node::UnlockedTree(tree) => tree.get(depth, key_bytes),
        }
    }
}

impl<K, V: ToMerkleBytes> Node<K, V> {
    pub(crate) fn lock(&mut self) -> (Sha256Hash, Arc<[u8]>) {
        match std::mem::take(self) {
            Node::Empty => empty(),
            Node::LockedLeaf(leaf) => {
                let ret = (leaf.hash, leaf.payload.clone());
                *self = Node::LockedLeaf(leaf);
                ret
            }
            Node::UnlockedLeaf(leaf) => {
                let leaf = leaf.lock();
                let ret = (leaf.hash, leaf.payload.clone());
                *self = Node::LockedLeaf(leaf);
                ret
            }
            Node::LockedTree(tree) => {
                let ret = (tree.hash, tree.payload.clone());
                *self = Node::LockedTree(tree);
                ret
            }
            Node::UnlockedTree(tree) => {
                let tree = tree.lock();
                let ret = (tree.hash, tree.payload.clone());
                *self = Node::LockedTree(tree);
                ret
            }
        }
    }
}

impl<K: FromMerkleBytes, V: FromMerkleBytes> Node<K, V> {
    pub(crate) fn load<Store: MerkleRead>(
        payload: Arc<[u8]>,
        manager: &MerkleManager<Store>,
    ) -> Result<Node<K, V>, LoadMerkleMapError<Store::Error>> {
        if payload.len() == 0 {
            return Err(LoadMerkleMapError::InsufficientInput);
        }
        match payload[0] {
            41 => {
                if payload.len() != 1 {
                    Err(LoadMerkleMapError::TooMuchInput)
                } else {
                    Ok(Node::Empty)
                }
            }
            42 => LeafContents::load::<Store>(payload).map(Node::LockedLeaf),
            43 => TreeContents::load(payload, manager).map(Node::LockedTree),
            byte => Err(LoadMerkleMapError::UnexpectedMagicByte { byte }),
        }
    }
}
