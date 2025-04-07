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

impl<K: Clone, V: Clone> Node<K, V> {
    pub(crate) fn unlock(self) -> UnlockedNode<K, V> {
        match self {
            Node::Empty => UnlockedNode::Leaf(LeafContents::default()),
            Node::LockedLeaf(leaf) => UnlockedNode::Leaf(leaf.into_inner()),
            Node::UnlockedLeaf(leaf) => UnlockedNode::Leaf(leaf),
            Node::LockedTree(tree) => UnlockedNode::Tree(tree.into_inner()),
            Node::UnlockedTree(tree) => UnlockedNode::Tree(*tree),
        }
    }

    pub(crate) fn get(&self, depth: u16, key_bytes: &MerkleKey) -> Option<&V> {
        match self {
            Node::Empty => None,
            Node::LockedLeaf(leaf) => leaf.inner.get(key_bytes),
            Node::UnlockedLeaf(leaf) => leaf.get(key_bytes),
            Node::LockedTree(tree) => tree.inner.get(depth, key_bytes),
            Node::UnlockedTree(tree) => tree.get(depth, key_bytes),
        }
    }

    pub(crate) fn get_mut(&mut self, depth: u16, key_bytes: &MerkleKey) -> Option<&mut V> {
        *self = match std::mem::take(self) {
            Node::LockedLeaf(leaf) => Node::UnlockedLeaf(leaf.into_inner()),
            Node::LockedTree(tree) => Node::UnlockedTree(Box::new(tree.into_inner())),
            other => other,
        };

        match self {
            Node::Empty => None,
            Node::LockedLeaf(_) => unreachable!(),
            Node::UnlockedLeaf(leaf) => leaf.get_mut(key_bytes),
            Node::LockedTree(_) => unreachable!(),
            Node::UnlockedTree(tree) => tree.get_mut(depth, key_bytes),
        }
    }
}

impl<K, V: MerkleSerialize> MerkleSerializeComplete for Node<K, V> {
    async fn serialize_complete<Store: MerkleStore>(
        &mut self,
        manager: &MerkleManager<Store>,
    ) -> Result<Sha256Hash, MerkleSerialError> {
        match std::mem::take(self) {
            Node::Empty => {
                let (hash, payload) = empty();
                manager.save_merkle_by_hash(hash, payload).await?;
                Ok(hash)
            }
            Node::LockedLeaf(leaf) => {
                manager
                    .save_merkle_by_hash(leaf.hash, leaf.payload.clone())
                    .await?;
                let hash = leaf.hash;
                *self = Node::LockedLeaf(leaf);
                Ok(hash)
            }
            Node::UnlockedLeaf(leaf) => {
                let leaf = leaf.lock(manager).await?;
                let hash = leaf.hash;
                *self = Node::LockedLeaf(leaf);
                Ok(hash)
            }
            Node::LockedTree(tree) => {
                manager
                    .save_merkle_by_hash(tree.hash, tree.payload.clone())
                    .await?;
                let hash = tree.hash;
                *self = Node::LockedTree(tree);
                Ok(hash)
            }
            Node::UnlockedTree(tree) => {
                let tree = tree.lock(manager).await?;
                let hash = tree.hash;
                *self = Node::LockedTree(tree);
                Ok(hash)
            }
        }
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> Node<K, V> {
    pub(crate) async fn load<Store: MerkleStore>(
        hash: Sha256Hash,
        payload: Arc<[u8]>,
        manager: &MerkleManager<Store>,
    ) -> Result<Node<K, V>, MerkleSerialError> {
        if payload.len() == 0 {
            return Err(MerkleSerialError::InsufficientInput);
        }
        let mut deserializer = manager.new_deserializer(&payload);

        match deserializer.pop_byte()? {
            41 => {
                deserializer.finish()?;
                Ok(Node::Empty)
            }
            42 => LeafContents::load(deserializer, hash, payload.clone()).map(Node::LockedLeaf),
            43 => TreeContents::load(deserializer, hash, payload.clone(), manager)
                .await
                .map(Node::LockedTree),
            byte => Err(MerkleSerialError::UnexpectedMagicByte { byte }),
        }
    }
}
