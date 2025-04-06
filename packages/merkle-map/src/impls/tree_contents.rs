use crate::*;

impl<K, V> TreeContents<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            len: 0,
            leaf: None,
            branches: std::array::from_fn(|_| Node::Empty),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

impl<K: ToMerkleBytes + Clone, V: Clone> TreeContents<K, V> {
    pub(crate) fn insert(&mut self, depth: u16, entry: LeafEntry<K, V>) -> Option<(K, V)> {
        let Some(index) = entry.key_bytes.get_index_for_depth(depth) else {
            debug_assert!(depth == 0 || entry.key_bytes.get_index_for_depth(depth - 1).is_some());
            let v = std::mem::replace(&mut self.leaf, Some(entry));
            if v.is_none() {
                self.len += 1;
            }
            return v.map(|entry| (entry.key, entry.value));
        };
        let index = usize::from(index);
        let (branch, v) = std::mem::take(&mut self.branches[index])
            .unlock()
            .insert(depth + 1, entry);
        let branch = branch.into();
        self.branches[index] = branch;
        if v.is_none() {
            self.len += 1;
        }
        v
    }

    pub(crate) fn get(&self, depth: u16, key_bytes: MerkleBytes) -> Option<&V> {
        let Some(index) = key_bytes.get_index_for_depth(depth) else {
            debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
            return self.leaf.as_ref().map(|entry| &entry.value);
        };
        debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
        let index = usize::from(index);
        self.branches[index].get(depth + 1, key_bytes)
    }

    pub(crate) fn remove(
        mut self,
        depth: u16,
        key_bytes: MerkleBytes,
    ) -> (UnlockedNode<K, V>, Option<(K, V)>) {
        let index = key_bytes
            .get_index_for_depth(depth)
            .expect("Impossible: TreeContents::remove without sufficient bytes");
        let index = usize::from(index);
        // FIXME check if we need to go back to a leaf because we have few enough nodes
        let branch = std::mem::take(&mut self.branches[index]).unlock();
        let (branch, v) = branch.remove(depth + 1, key_bytes);
        self.branches[index] = branch.into();
        if v.is_some() {
            self.len -= 1;
        }
        (UnlockedNode::Tree(self), v)
    }

    pub(crate) fn drain_entries_to(self, entries: &mut Vec<LeafEntry<K, V>>) {
        if let Some(entry) = self.leaf {
            entries.push(entry);
        }
        for branch in self.branches {
            match branch {
                Node::Empty => (),
                Node::LockedLeaf(leaf) => leaf.into_inner().drain_entries_to(entries),
                Node::UnlockedLeaf(leaf) => leaf.drain_entries_to(entries),
                Node::LockedTree(tree) => tree.into_inner().drain_entries_to(entries),
                Node::UnlockedTree(tree) => tree.drain_entries_to(entries),
            }
        }
    }
}

impl<K, V: ToMerkleBytes> TreeContents<K, V> {
    pub(crate) fn lock(mut self) -> Locked<TreeContents<K, V>> {
        let mut buff = WriteBuffer::from(vec![43]);
        buff.store_usize(self.len);
        match &self.leaf {
            Some(leaf) => {
                buff.push(1);
                leaf.store(&mut buff);
            }
            None => buff.push(0),
        }
        for branch in &mut self.branches {
            let (hash, _) = branch.lock();
            buff.extend_from_slice(hash.as_array());
        }
        Locked::new(buff.finish(), self)
    }
}

impl<K: FromMerkleBytes, V: FromMerkleBytes> TreeContents<K, V> {
    pub(crate) fn load<Store: MerkleRead>(
        payload: Arc<[u8]>,
        manager: &MerkleManager<Store>,
    ) -> Result<Locked<TreeContents<K, V>>, LoadMerkleMapError<Store::Error>> {
        let mut buff = ReadBuffer::new(&payload);
        assert_eq!(buff.pop_byte()?, 43);
        let len = buff.load_usize()?;
        let leaf = match buff.pop_byte()? {
            0 => None,
            1 => Some(LeafEntry::load::<Store>(&mut buff)?),
            byte => return Err(LoadMerkleMapError::InvalidTreeStart { byte }),
        };
        let mut branches = std::array::from_fn(|_| Node::Empty);
        for branch in &mut branches {
            let hash = buff.load_hash()?;
            *branch = manager
                .load(hash)?
                .ok_or(LoadMerkleMapError::HashNotFound { hash })?
                .0;
        }
        buff.finish()?;
        let tree = TreeContents {
            len,
            leaf,
            branches,
        };
        Ok(Locked::new(payload, tree))
    }
}
