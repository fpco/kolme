use std::fmt::Debug;

use crate::*;

impl<K, V> Clone for Node<K, V> {
    fn clone(&self) -> Self {
        Node(self.0.clone())
    }
}

impl<K, V> Default for Node<K, V> {
    fn default() -> Self {
        Node(MerkleLockable::new(NodeContents::Empty))
    }
}

impl<K, V> Node<K, V> {
    pub(crate) fn is_empty(&self) -> bool {
        let res = self.0.as_ref().is_empty();
        #[cfg(debug_assertions)]
        assert_eq!(res, self.len() == 0);
        res
    }

    pub(crate) fn len(&self) -> usize {
        self.0.as_ref().len()
    }
}

impl<K, V> Node<K, V> {
    pub(crate) fn get(&self, key_bytes: &MerkleKey) -> Option<&LeafEntry<K, V>> {
        self.0.as_ref().get(key_bytes)
    }
}

impl<K: Clone, V: Clone> Node<K, V> {
    pub(crate) fn get_mut(&mut self, depth: u16, key_bytes: &MerkleKey) -> Option<&mut V> {
        self.0.as_mut().get_mut(depth, key_bytes)
    }
}

impl<K: Clone, V: Clone> Node<K, V> {
    pub(crate) fn insert(self, depth: u16, entry: LeafEntry<K, V>) -> (Node<K, V>, Option<(K, V)>) {
        let contents = self.0.into_inner();
        let (contents, old) = contents.insert(depth, entry);
        (
            Node(MerkleLockable::new(contents)),
            old.map(|entry| (entry.key, entry.value)),
        )
    }

    pub(crate) fn remove(self, depth: u16, key_bytes: MerkleKey) -> (Node<K, V>, Option<(K, V)>) {
        let contents = self.0.into_inner();
        let (contents, old) = contents.remove(depth, &key_bytes);
        (Node(MerkleLockable::new(contents)), old)
    }
}

impl<K, V> MerkleSerializeRaw for Node<K, V>
where
    K: ToMerkleKey + Send + Sync + 'static,
    V: MerkleSerializeRaw + Send + Sync + 'static,
{
    fn get_merkle_hash_raw(&self) -> Option<Sha256Hash> {
        self.0.get_merkle_hash_raw()
    }

    fn set_merkle_hash_raw(&self, hash: Sha256Hash) {
        self.0.set_merkle_hash_raw(hash);
    }

    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        self.0.merkle_serialize_raw(serializer)
    }
}

impl<K, V> MerkleDeserializeRaw for Node<K, V>
where
    K: FromMerkleKey + Send + Sync + 'static,
    V: MerkleDeserializeRaw + Send + Sync + 'static,
{
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Node(MerkleLockable::new(
            NodeContents::merkle_deserialize_raw(deserializer)?,
        )))
    }

    // FIXME do we need to implement load_merkle_by_hash?
}

impl<K: Debug, V: Debug> Debug for Node<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}
