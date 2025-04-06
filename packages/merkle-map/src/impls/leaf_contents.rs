use crate::*;

impl<K: ToMerkleBytes + Clone, V: Clone> From<TreeContents<K, V>> for LeafContents<K, V> {
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
    ) -> (UnlockedNode<K, V>, Option<(K, V)>)
    where
        K: ToMerkleBytes + Clone,
        V: Clone,
    {
        // Try to do a replace
        for old_entry in &mut self.values {
            if old_entry.key_bytes == entry.key_bytes {
                std::mem::swap(old_entry, &mut entry);
                return (UnlockedNode::Leaf(self), Some((entry.key, entry.value)));
            }
        }

        if self.values.len() < 16 {
            self.values.push(entry);
            self.sort();
            (UnlockedNode::Leaf(self), None)
        } else {
            let mut tree = TreeContents::new();
            tree.insert(depth, entry);
            self.values.into_iter().for_each(|entry| {
                let old = tree.insert(depth, entry);
                assert!(old.is_none());
            });
            (UnlockedNode::Tree(tree), None)
        }
    }

    pub(crate) fn get<Q>(&self, key_bytes: MerkleBytes) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: ToMerkleBytes + ?Sized,
    {
        self.values.iter().find_map(|entry| {
            if entry.key_bytes == key_bytes {
                // FIXME check equality of key too? Or not necessary?
                Some(&entry.value)
            } else {
                None
            }
        })
    }

    pub(crate) fn remove<Q>(&mut self, key_bytes: MerkleBytes) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: ToMerkleBytes + ?Sized,
    {
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
                Some((key, value))
            }
            None => None,
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

impl<K, V: ToMerkleBytes> LeafContents<K, V> {
    pub(crate) fn lock(self) -> Locked<LeafContents<K, V>> {
        let mut buff = WriteBuffer::from(vec![42]);
        buff.store_usize(self.values.len());
        for entry in &self.values {
            entry.store(&mut buff);
        }
        Locked::new(buff.finish(), self)
    }
}

impl<K: FromMerkleBytes, V: FromMerkleBytes> LeafContents<K, V> {
    pub(crate) fn load<Store: MerkleRead>(
        payload: Arc<[u8]>,
    ) -> Result<Locked<LeafContents<K, V>>, LoadMerkleMapError<Store::Error>> {
        let mut buff = ReadBuffer::new(&payload);
        assert_eq!(buff.pop_byte()?, 42);
        let len = buff.load_usize()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(LeafEntry::load::<Store>(&mut buff)?);
        }
        buff.finish()?;

        Ok(Locked::new(payload, LeafContents { values }))
    }
}
