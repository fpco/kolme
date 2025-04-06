use crate::*;

impl<K, V> MerkleTree<K, V> {
    pub fn new() -> Self {
        MerkleTree(Node::Empty)
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

impl<K, V> MerkleTree<K, V>
where
    K: MerkleKey + Clone,
    V: Clone,
{
    pub fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        self.sanity_checks();
        let key_bytes = key.to_bytes();
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

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        self.sanity_checks();
        self.0.get(0, key.to_bytes())
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        self.sanity_checks();
        let node = std::mem::take(&mut self.0).unlock();
        let (node, v) = node.remove(0, key.to_bytes());
        self.0 = node.into();
        self.sanity_checks();
        v
    }

    pub fn pop_first(&mut self) -> Option<(K, V)> {
        self.sanity_checks();
        let res = self
            .find_first()
            .cloned() // FIXME optimize this away
            .and_then(|first| self.remove(&first));
        self.sanity_checks();
        res
    }

    pub fn find_first(&self) -> Option<&K> {
        self.sanity_checks();
        match &self.0 {
            Node::Empty => None,
            Node::LockedLeaf(leaf) => leaf.0.inner.find_first(),
            Node::UnlockedLeaf(leaf) => leaf.find_first(),
            Node::LockedTree(tree) => tree.0.inner.find_first(),
            Node::UnlockedTree(tree) => tree.find_first(),
        }
    }

    pub fn iter(&self) -> Iter<K, V> {
        self.sanity_checks();
        self.into_iter()
    }
}

impl<K, V> Default for MerkleTree<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
