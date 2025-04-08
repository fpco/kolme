use shared::types::Sha256Hash;

use crate::*;

impl<K, V> Node<K, V> {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Node::Empty => true,
            Node::Leaf(_) | Node::Tree(_) => false,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Node::Empty => 0,
            Node::Leaf(leaf) => leaf.as_ref().len(),
            Node::Tree(tree) => tree.as_ref().len(),
        }
    }
}

impl<K: Clone, V: Clone> Node<K, V> {
    pub(crate) fn get(&self, depth: u16, key_bytes: &MerkleKey) -> Option<&V> {
        match self {
            Node::Empty => None,
            Node::Leaf(leaf) => leaf.as_ref().get(key_bytes),
            Node::Tree(tree) => tree.as_ref().get(depth, key_bytes),
        }
    }

    pub(crate) fn get_mut(&mut self, depth: u16, key_bytes: &MerkleKey) -> Option<&mut V> {
        match self {
            Node::Empty => None,
            Node::Leaf(leaf) => leaf.as_mut().get_mut(key_bytes),
            Node::Tree(tree) => tree.as_mut().get_mut(depth, key_bytes),
        }
    }
}

impl<K: Clone, V: Clone> Node<K, V> {
    pub(crate) fn insert(self, depth: u16, entry: LeafEntry<K, V>) -> (Node<K, V>, Option<(K, V)>) {
        match self {
            Node::Empty => LeafContents::default().insert(depth, entry),
            Node::Leaf(leaf) => leaf.into_inner().insert(depth, entry),
            Node::Tree(mut tree) => {
                let v = tree.as_mut().insert(depth, entry);
                (Node::Tree(tree), v)
            }
        }
    }

    pub(crate) fn remove(self, depth: u16, key_bytes: MerkleKey) -> (Node<K, V>, Option<(K, V)>) {
        match self {
            Node::Empty => (self, None),
            Node::Leaf(leaf) => leaf.into_inner().remove(key_bytes),
            Node::Tree(tree) => tree.into_inner().remove(depth, key_bytes),
        }
    }
}

// impl<K, V: MerkleSerialize> MerkleSerializeComplete for Node<K, V> {
//     async fn serialize_complete<Store: MerkleStore>(
//         &mut self,
//         manager: &MerkleManager<Store>,
//     ) -> Result<Sha256Hash, MerkleSerialError> {
//         match std::mem::take(self) {
//             Node::Empty => {
//                 let (hash, payload) = empty();
//                 manager.save_merkle_by_hash(hash, payload).await?;
//                 Ok(hash)
//             }
//             Node::LockedLeaf(leaf) => {
//                 manager
//                     .save_merkle_by_hash(leaf.hash, leaf.payload.clone())
//                     .await?;
//                 let hash = leaf.hash;
//                 *self = Node::LockedLeaf(leaf);
//                 Ok(hash)
//             }
//             Node::UnlockedLeaf(leaf) => {
//                 let leaf = leaf.lock(manager).await?;
//                 let hash = leaf.hash;
//                 *self = Node::LockedLeaf(leaf);
//                 Ok(hash)
//             }
//             Node::LockedTree(tree) => {
//                 manager
//                     .save_merkle_by_hash(tree.hash, tree.payload.clone())
//                     .await?;
//                 let hash = tree.hash;
//                 *self = Node::LockedTree(tree);
//                 Ok(hash)
//             }
//             Node::UnlockedTree(tree) => {
//                 let tree = tree.lock(manager).await?;
//                 let hash = tree.hash;
//                 *self = Node::LockedTree(tree);
//                 Ok(hash)
//             }
//         }
//     }
// }
// FIXME

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
            42 => LeafContents::load(deserializer, hash, payload.clone()).map(Node::Leaf),
            43 => TreeContents::load(deserializer, hash, payload.clone(), manager)
                .await
                .map(Node::Tree),
            byte => Err(MerkleSerialError::UnexpectedMagicByte { byte }),
        }
    }
}
