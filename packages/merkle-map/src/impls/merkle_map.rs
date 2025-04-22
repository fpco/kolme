use std::fmt::Debug;

use crate::*;

impl<K: Clone, V: Clone> Clone for MerkleMap<K, V> {
    fn clone(&self) -> Self {
        MerkleMap(self.0.clone())
    }
}

impl<K, V> MerkleMap<K, V> {
    pub fn new() -> Self {
        MerkleMap(Node::default())
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
        let node = std::mem::take(&mut self.0);
        let (node, v) = node.insert(
            0,
            LeafEntry {
                key_bytes,
                key,
                value,
            },
        );
        self.0 = node;
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
        self.0.get(0, &key.to_merkle_key())
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ToMerkleKey + ?Sized,
    {
        self.get(key).is_some()
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: ToMerkleKey + ?Sized,
    {
        self.sanity_checks();
        self.0.get_mut(0, &key.to_merkle_key())
    }
}

impl<K, V> MerkleMap<K, V>
where
    K: ToMerkleKey + Clone,
    V: Clone,
{
    pub fn get_or_insert(&mut self, key: K, def: impl FnOnce() -> V) -> &mut V {
        self.sanity_checks();
        // TODO could optimize this
        let key_bytes = key.to_merkle_key();
        if self.0.get(0, &key_bytes).is_some() {
            return self.0.get_mut(0, &key_bytes).unwrap();
        }
        self.insert(key, def());
        self.0.get_mut(0, &key_bytes).unwrap()
    }
}

impl<K, V> MerkleMap<K, V>
where
    K: ToMerkleKey + Clone,
    V: Default + Clone,
{
    pub fn get_or_default(&mut self, key: K) -> &mut V {
        self.get_or_insert(key, Default::default)
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
        let node = std::mem::take(&mut self.0);
        let (node, v) = node.remove(0, key.to_merkle_key());
        self.0 = node;
        self.sanity_checks();
        v
    }
}

impl<K, V> MerkleMap<K, V> {
    pub fn iter(&self) -> crate::impls::iter::Iter<K, V> {
        self.sanity_checks();
        self.into_iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(k, _v)| k)
    }
}

impl<K, V> Default for MerkleMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Debug, V: Debug> Debug for MerkleMap<K, V> {
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

impl<K: ToMerkleKey, V: MerkleSerialize> MerkleSerialize for MerkleMap<K, V> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        self.0.merkle_serialize(serializer)
    }

    fn get_merkle_contents(&self) -> Option<Arc<MerkleContents>> {
        self.0.get_merkle_contents()
    }

    fn set_merkle_contents(&self, contents: &Arc<MerkleContents>) {
        self.0.set_merkle_contents(contents)
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> MerkleDeserialize for MerkleMap<K, V> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Node::merkle_deserialize(deserializer).map(MerkleMap)
    }
}

impl<K: ToMerkleKey + Clone, V: Clone> FromIterator<(K, V)> for MerkleMap<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut m = MerkleMap::new();
        for (k, v) in iter {
            m.insert(k, v);
        }
        m
    }
}
