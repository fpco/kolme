use crate::*;

impl<K, V> MerkleMap<K, V> {
    pub fn new() -> Self {
        MerkleMap(Node::Empty)
    }

    pub fn is_empty(&self) -> bool {
        self.sanity_checks();
        self.0.is_empty()
    }

    fn sanity_checks(&self) {
        #[cfg(test)]
        self.0.sanity_checks();
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl<K, V> MerkleMap<K, V>
where
    K: ToMerkleKey + Clone,
    V: Clone,
{
    pub fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        self.sanity_checks();
        let key_bytes = key.to_merkle_key();
        let node = std::mem::take(&mut self.0).unlock();
        let (node, v) = node.insert(
            0,
            LeafEntry {
                key_bytes,
                key,
                value,
            },
        );
        self.0 = node.into();
        self.sanity_checks();
        v
    }
}

impl<K, V> MerkleMap<K, V>
where
    K: Clone,
    V: Clone,
{
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ToMerkleKey + ?Sized,
    {
        self.sanity_checks();
        self.0.get(0, key.to_merkle_key())
    }
}

impl<K, V> MerkleMap<K, V>
where
    K: Clone,
    V: Clone,
{
    pub fn remove<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: ToMerkleKey + ?Sized,
    {
        self.sanity_checks();
        let node = std::mem::take(&mut self.0).unlock();
        let (node, v) = node.remove(0, key.to_merkle_key());
        self.0 = node.into();
        self.sanity_checks();
        v
    }

    pub fn iter(&self) -> crate::impls::iter::Iter<K, V> {
        self.sanity_checks();
        self.into_iter()
    }
}

impl<K, V> Default for MerkleMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V: MerkleSerialize> MerkleSerializeComplete for MerkleMap<K, V> {
    fn serialize_complete<Store: MerkleStore>(
        &mut self,
        manager: &MerkleManager<Store>,
    ) -> Result<shared::types::Sha256Hash, MerkleSerialError> {
        self.0.serialize_complete(manager)
    }
}
