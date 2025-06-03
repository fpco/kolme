use std::collections::HashSet;

use sha2::{Digest, Sha256};

use crate::*;

impl<K, V> TreeContents<K, V> {
    pub(crate) fn new() -> Self {
        Self {
            len: 0,
            leaf: None,
            branches: std::array::from_fn(|_| Node::default()),
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
        self.branches[index] = branch;
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
        self.branches[index].get(depth + 1, key_bytes)
    }
    pub(crate) fn hash(&self) -> Sha256Hash {
        let mut hasher = Sha256::new();
        if let Some(leaf) = &self.leaf {
            hasher.update(leaf.hash().as_array());
        }
        for branch in self.branches.iter() {
            if !Node::is_empty(branch) {
                hasher.update(branch.hash().as_array());
            }
        }
        Sha256Hash::from_array(hasher.finalize().into())
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
        let branch = std::mem::take(&mut self.branches[index]);
        let (branch, v) = branch.remove(depth + 1, key_bytes);
        self.branches[index] = branch;
        if v.is_some() {
            self.len -= 1;
        }
        let node = if self.len <= 16 {
            let mut values = arrayvec::ArrayVec::new();
            self.drain_entries_to(&mut values);
            Node::Leaf(MerkleLockable::new(LeafContents { values }))
        } else {
            Node::Tree(MerkleLockable::new(self))
        };
        (node, v)
    }

    pub(crate) fn drain_entries_to(self, entries: &mut arrayvec::ArrayVec<LeafEntry<K, V>, 16>) {
        if let Some(entry) = self.leaf {
            entries.push(entry);
        }
        for branch in self.branches {
            match branch {
                Node::Leaf(leaf) => leaf.into_inner().drain_entries_to(entries),
                Node::Tree(tree) => tree.into_inner().drain_entries_to(entries),
            }
        }
    }
}

impl<K: ToMerkleKey, V: MerkleSerialize> MerkleSerialize for TreeContents<K, V> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_byte(43);
        serializer.store_usize(self.len);
        match &self.leaf {
            Some(leaf) => {
                serializer.store_byte(1);
                leaf.merkle_serialize(serializer)?;
            }
            None => serializer.store_byte(0),
        }
        for branch in &self.branches {
            serializer.store_by_hash(branch)?;
        }
        Ok(())
    }
}

impl<K: FromMerkleKey, V: MerkleDeserialize> MerkleDeserialize
    for MerkleLockable<TreeContents<K, V>>
{
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let magic_byte = deserializer.pop_byte()?;
        if magic_byte != 43 {
            return Err(MerkleSerialError::UnexpectedMagicByte { byte: magic_byte });
        }
        let len = deserializer.load_usize()?;
        let leaf = match deserializer.pop_byte()? {
            0 => None,
            1 => Some(LeafEntry::merkle_deserialize(deserializer)?),
            byte => return Err(MerkleSerialError::InvalidTreeStart { byte }),
        };
        let mut branches = std::array::from_fn(|_| Node::default());
        let mut missing = HashSet::new();
        for branch in &mut branches {
            let hash = Sha256Hash::merkle_deserialize(deserializer)?;
            match deserializer.load_by_hash_optional(hash)? {
                Some(value) => *branch = value,
                None => {
                    missing.insert(hash);
                }
            }
        }
        if !missing.is_empty() {
            return Err(MerkleSerialError::HashesNotFound { hashes: missing });
        }
        let tree = TreeContents {
            len,
            leaf,
            branches,
        };
        Ok(MerkleLockable::new(tree))
    }
}
