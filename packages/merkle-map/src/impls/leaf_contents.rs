use crate::*;

impl<K: Clone, V: Clone> From<TreeContents<K, V>> for LeafContents<K, V> {
    fn from(tree: TreeContents<K, V>) -> Self {
        assert!(tree.len() <= 16);
        let mut leaf = LeafContents { values: vec![] };
        tree.drain_entries_to(&mut leaf.values);
        leaf
    }
}

impl<K, V> LeafContents<K, V> {
    fn sort(&mut self) {
        self.values.sort_by(|x, y| x.key_bytes.cmp(&y.key_bytes));
    }

    pub(crate) fn insert(
        mut self,
        depth: u16,
        mut entry: LeafEntry<K, V>,
    ) -> (Node<K, V>, Option<(K, V)>)
    where
        K: Clone,
        V: Clone,
    {
        // Try to do a replace
        for old_entry in &mut self.values {
            if old_entry.key_bytes == entry.key_bytes {
                std::mem::swap(old_entry, &mut entry);
                return (
                    Node::Leaf(Lockable::new_unlocked(self)),
                    Some((entry.key, entry.value)),
                );
            }
        }

        if self.values.len() < 16 {
            self.values.push(entry);
            self.sort();
            (Node::Leaf(Lockable::new_unlocked(self)), None)
        } else {
            let mut tree = TreeContents::new();
            tree.insert(depth, entry);
            self.values.into_iter().for_each(|entry| {
                let old = tree.insert(depth, entry);
                assert!(old.is_none());
            });
            (Node::Tree(Lockable::new_unlocked(tree)), None)
        }
    }

    pub(crate) fn get(&self, key_bytes: &MerkleKey) -> Option<&LeafEntry<K, V>> {
        self.values.iter().find_map(|entry| {
            if &entry.key_bytes == key_bytes {
                Some(entry)
            } else {
                None
            }
        })
    }

    pub(crate) fn get_mut(&mut self, key_bytes: &MerkleKey) -> Option<&mut V> {
        self.values.iter_mut().find_map(|entry| {
            if &entry.key_bytes == key_bytes {
                Some(&mut entry.value)
            } else {
                None
            }
        })
    }

    pub(crate) fn remove(mut self, key_bytes: MerkleKey) -> (Node<K, V>, Option<(K, V)>) {
        match self.values.iter().enumerate().find_map(|(idx, entry)| {
            if entry.key_bytes == key_bytes {
                Some(idx)
            } else {
                None
            }
        }) {
            Some(idx) => {
                let LeafEntry {
                    key_bytes: _,
                    key,
                    value,
                } = self.values.remove(idx);
                let node = Node::Leaf(Lockable::new_unlocked(self));
                (node, Some((key, value)))
            }
            None => (Node::Leaf(Lockable::new_unlocked(self)), None),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn drain_entries_to(mut self, entries: &mut Vec<LeafEntry<K, V>>) {
        entries.append(&mut self.values);
    }
}

impl<K, V> Default for LeafContents<K, V> {
    fn default() -> Self {
        Self { values: vec![] }
    }
}

impl<K, V: MerkleSerialize> MerkleSerialize for LeafContents<K, V> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_byte(42);
        serializer.store_usize(self.values.len());
        for entry in &self.values {
            entry.merkle_serialize(serializer)?;
        }

        Ok(())
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> MerkleDeserialize for Lockable<LeafContents<K, V>> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let magic_byte = deserializer.pop_byte()?;
        if magic_byte != 42 {
            return Err(MerkleSerialError::UnexpectedMagicByte { byte: magic_byte });
        }
        let len = deserializer.load_usize()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(LeafEntry::merkle_deserialize(deserializer)?);
        }

        Ok(Lockable::new_unlocked(LeafContents { values }))
    }
}
