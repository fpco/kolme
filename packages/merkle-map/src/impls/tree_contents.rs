use shared::types::Sha256Hash;

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

impl<K: Clone, V: Clone> TreeContents<K, V> {
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
        let (branch, v) = std::mem::take(&mut self.branches[index]).insert(depth + 1, entry);
        let branch = branch.into();
        self.branches[index] = branch;
        if v.is_none() {
            self.len += 1;
        }
        v
    }

    pub(crate) fn get(&self, depth: u16, key_bytes: &MerkleKey) -> Option<&V> {
        let Some(index) = key_bytes.get_index_for_depth(depth) else {
            debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
            return self.leaf.as_ref().map(|entry| &entry.value);
        };
        debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
        let index = usize::from(index);
        self.branches[index].get(depth + 1, key_bytes)
    }

    pub(crate) fn get_mut(&mut self, depth: u16, key_bytes: &MerkleKey) -> Option<&mut V> {
        let Some(index) = key_bytes.get_index_for_depth(depth) else {
            debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
            return self.leaf.as_mut().map(|entry| &mut entry.value);
        };
        debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
        let index = usize::from(index);
        self.branches[index].get_mut(depth + 1, key_bytes)
    }

    pub(crate) fn remove(
        mut self,
        depth: u16,
        key_bytes: MerkleKey,
    ) -> (Node<K, V>, Option<(K, V)>) {
        let index = key_bytes
            .get_index_for_depth(depth)
            .expect("Impossible: TreeContents::remove without sufficient bytes");
        let index = usize::from(index);
        // FIXME check if we need to go back to a leaf because we have few enough nodes
        let branch = std::mem::take(&mut self.branches[index]);
        let (branch, v) = branch.remove(depth + 1, key_bytes);
        self.branches[index] = branch.into();
        if v.is_some() {
            self.len -= 1;
        }
        (Node::Tree(Lockable::new_unlocked(self)), v)
    }

    pub(crate) fn drain_entries_to(self, entries: &mut Vec<LeafEntry<K, V>>) {
        if let Some(entry) = self.leaf {
            entries.push(entry);
        }
        for branch in self.branches {
            match branch {
                Node::Empty => (),
                Node::Leaf(leaf) => leaf.into_inner().drain_entries_to(entries),
                Node::Tree(tree) => tree.into_inner().drain_entries_to(entries),
            }
        }
    }
}

impl<K, V: MerkleSerialize> TreeContents<K, V> {
    pub(crate) async fn lock<Store: MerkleStore>(
        mut self,
        manager: &MerkleManager<Store>,
    ) -> Result<Lockable<TreeContents<K, V>>, MerkleSerialError> {
        let mut serializer = manager.new_serializer();
        serializer.store_byte(43);
        serializer.store_usize(self.len);
        match &mut self.leaf {
            Some(leaf) => {
                serializer.store_byte(1);
                leaf.serialize(&mut serializer).await?;
            }
            None => serializer.store_byte(0),
        }
        for _branch in &mut self.branches {
            serializer.new_serializer();
            todo!()
            // let mut hash = branch.serialize_complete(manager).await?;
            // hash.serialize(&mut serializer).await?;
        }
        let (hash, payload) = serializer.finish().await?;
        Ok(Lockable::new_locked(hash, payload, self))
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> TreeContents<K, V> {
    pub(crate) async fn load<Store: MerkleStore, D: MerkleDeserializer>(
        mut deserializer: D,
        hash: Sha256Hash,
        payload: Arc<[u8]>,
        manager: &MerkleManager<Store>,
    ) -> Result<Lockable<TreeContents<K, V>>, MerkleSerialError> {
        // Byte 43 already checked in caller
        let len = deserializer.load_usize()?;
        let leaf = match deserializer.pop_byte()? {
            0 => None,
            1 => Some(LeafEntry::deserialize(&mut deserializer)?),
            byte => return Err(MerkleSerialError::InvalidTreeStart { byte }),
        };
        let mut branches = std::array::from_fn(|_| Node::Empty);
        for branch in &mut branches {
            let hash = Sha256Hash::deserialize(&mut deserializer)?;
            *branch = manager
                .load(hash)
                .await?
                .ok_or(MerkleSerialError::HashNotFound { hash })?
                .0;
        }
        deserializer.finish()?;
        let tree = TreeContents {
            len,
            leaf,
            branches,
        };
        Ok(Lockable::new_locked(hash, payload, tree))
    }
}
