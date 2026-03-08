use crate::*;

struct EmptyBranchSerialize;

impl MerkleSerializeRaw for EmptyBranchSerialize {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_byte(42);
        serializer.store_usize(0);
        Ok(())
    }
}

const EMPTY_BRANCH_SERIALIZE: EmptyBranchSerialize = EmptyBranchSerialize;

impl<K, V> TreeContents<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            len: 0,
            leaf: None,
            branches: std::array::from_fn(|_| None),
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
            let v = self.leaf.replace(entry);
            if v.is_none() {
                self.len += 1;
            }
            return v.map(|entry| (entry.key, entry.value));
        };
        let index = usize::from(index);
        let branch_slot = &mut self.branches[index];
        let current = branch_slot.take().unwrap_or_default();
        let (branch, v) = current.insert(depth + 1, entry);
        *branch_slot = Some(branch);
        if v.is_none() {
            self.len += 1;
        }
        v
    }
}

impl<K, V> TreeContents<K, V> {
    pub(crate) fn get(&self, depth: u16, key_bytes: &MerkleKey) -> Option<&LeafEntry<K, V>> {
        let Some(index) = key_bytes.get_index_for_depth(depth) else {
            debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
            return self.leaf.as_ref();
        };
        debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
        let index = usize::from(index);
        self.branches[index]
            .as_ref()
            .and_then(|branch| branch.get(depth + 1, key_bytes))
    }
}

impl<K: Clone, V: Clone> TreeContents<K, V> {
    pub(crate) fn get_mut(&mut self, depth: u16, key_bytes: &MerkleKey) -> Option<&mut V> {
        let Some(index) = key_bytes.get_index_for_depth(depth) else {
            debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
            return self.leaf.as_mut().map(|entry| &mut entry.value);
        };
        debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
        let index = usize::from(index);
        self.branches[index]
            .as_mut()
            .and_then(|branch| branch.get_mut(depth + 1, key_bytes))
    }

    pub(crate) fn remove(
        mut self,
        depth: u16,
        key_bytes: MerkleKey,
    ) -> (Node<K, V>, Option<(K, V)>) {
        let Some(index) = key_bytes.get_index_for_depth(depth) else {
            debug_assert!(depth == 0 || key_bytes.get_index_for_depth(depth - 1).is_some());
            if let Some(entry) = self.leaf.take() {
                if entry.key_bytes == key_bytes {
                    self.len -= 1;
                    let node = self.into_node_after_removal();
                    return (node, Some((entry.key, entry.value)));
                } else {
                    self.leaf = Some(entry);
                }
            }
            return (Node::Tree(MerkleLockable::new(self)), None);
        };
        let index = usize::from(index);
        let Some(branch) = self.branches[index].take() else {
            return (Node::Tree(MerkleLockable::new(self)), None);
        };
        let (branch, v) = branch.remove(depth + 1, key_bytes);
        if !branch.is_empty() {
            self.branches[index] = Some(branch);
        }
        if v.is_some() {
            self.len -= 1;
            let node = self.into_node_after_removal();
            (node, v)
        } else {
            (Node::Tree(MerkleLockable::new(self)), None)
        }
    }

    fn into_node_after_removal(self) -> Node<K, V> {
        if self.len <= 16 {
            let mut values = Vec::new();
            self.drain_entries_to(&mut values);
            Node::Leaf(MerkleLockable::new(LeafContents { values }))
        } else {
            Node::Tree(MerkleLockable::new(self))
        }
    }

    pub(crate) fn drain_entries_to(self, entries: &mut Vec<LeafEntry<K, V>>) {
        if let Some(entry) = self.leaf {
            entries.push(entry);
        }
        for branch in self.branches.into_iter().flatten() {
            match branch {
                Node::Leaf(leaf) => leaf.into_inner().drain_entries_to(entries),
                Node::Tree(tree) => tree.into_inner().drain_entries_to(entries),
            }
        }
    }
}

impl<K, V> MerkleSerializeRaw for TreeContents<K, V>
where
    K: ToMerkleKey + Send + Sync + 'static,
    V: MerkleSerializeRaw + Send + Sync + 'static,
{
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_byte(43);
        serializer.store_usize(self.len);
        match &self.leaf {
            Some(leaf) => {
                serializer.store_byte(1);
                leaf.merkle_serialize_raw(serializer)?;
            }
            None => serializer.store_byte(0),
        }
        for branch in &self.branches {
            match branch {
                Some(branch) => serializer.store_by_hash(branch)?,
                None => serializer.store_by_hash(&EMPTY_BRANCH_SERIALIZE)?,
            }
        }
        Ok(())
    }
}

impl<K, V> MerkleDeserializeRaw for TreeContents<K, V>
where
    K: FromMerkleKey + Send + Sync + 'static,
    V: MerkleDeserializeRaw + Send + Sync + 'static,
{
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let magic_byte = deserializer.pop_byte()?;
        if magic_byte != 43 {
            return Err(MerkleSerialError::UnexpectedMagicByte { byte: magic_byte });
        }
        let len = deserializer.load_usize()?;
        let leaf = match deserializer.pop_byte()? {
            0 => None,
            1 => Some(LeafEntry::merkle_deserialize_raw(deserializer)?),
            byte => return Err(MerkleSerialError::InvalidTreeStart { byte }),
        };
        let mut branches = std::array::from_fn(|_| None);
        for branch in &mut branches {
            let hash = Sha256Hash::merkle_deserialize_raw(deserializer)?;
            let node: Node<K, V> = deserializer.load_by_given_hash(hash)?;
            if !node.is_empty() {
                *branch = Some(node);
            }
        }
        let tree = TreeContents {
            len,
            leaf,
            branches,
        };
        Ok(tree)
    }
}
