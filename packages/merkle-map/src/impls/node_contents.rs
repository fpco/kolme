use crate::*;

impl<K, V> Clone for NodeContents<K, V> {
    fn clone(&self) -> Self {
        todo!()
    }
}

impl<K, V> NodeContents<K, V> {
    pub(crate) fn is_empty(&self) -> bool {
        matches!(self, NodeContents::Empty)
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            NodeContents::Empty => 0,
            NodeContents::Leaf(entries) => entries.len(),
            NodeContents::TreeWithLeaf { len, .. } | NodeContents::TreeWithoutLeaf { len, .. } => {
                *len
            }
        }
    }

    pub(crate) fn get(&self, key_bytes: &MerkleKey) -> Option<&LeafEntry<K, V>> {
        let mut contents = self;
        let mut depth = 0;
        loop {
            match contents {
                NodeContents::Empty => break None,
                NodeContents::Leaf(entries) => {
                    return entries.iter().find(|entry| &entry.key_bytes == key_bytes)
                }
                NodeContents::TreeWithLeaf {
                    len,
                    leaf,
                    branches,
                } => {
                    let Some(index) = key_bytes.get_index_for_depth(depth) else {
                        return Some(leaf);
                    };
                    let node = branches.iter().find(|node| node.halfbyte == index)?;
                    contents = node.node.0.as_ref();
                    depth += 1;
                }
                NodeContents::TreeWithoutLeaf { len, branches } => {
                    let index = key_bytes.get_index_for_depth(depth)?;
                    let node = branches.iter().find(|node| node.halfbyte == index)?;
                    contents = node.node.0.as_ref();
                    depth += 1;
                }
            }
        }
    }

    pub(crate) fn get_mut(&mut self, depth: u16, key_bytes: &MerkleKey) -> Option<&mut V> {
        todo!()
    }

    pub(crate) fn insert(
        self,
        depth: u16,
        mut entry: LeafEntry<K, V>,
    ) -> (Self, Option<LeafEntry<K, V>>) {
        let mut make_tree = match self {
            NodeContents::Empty => return (NodeContents::Leaf(vec![entry]), None),
            NodeContents::Leaf(mut entries) => {
                if let Some(old) = entries
                    .iter_mut()
                    .find(|existing_entry| existing_entry.key_bytes == entry.key_bytes)
                {
                    std::mem::swap(old, &mut entry);
                    return (NodeContents::Leaf(entries), Some(entry));
                }

                if entries.len() < 16 {
                    entries.push(entry);
                    entries.sort_unstable_by(|x, y| x.key_bytes.cmp(&y.key_bytes));
                    return (NodeContents::Leaf(entries), None);
                }

                let mut make_tree = MakeTree {
                    depth,
                    len: 0,
                    branches: vec![],
                    leaf: None,
                };
                for entry in entries.into_iter().chain(std::iter::once(entry)) {
                    let old = make_tree.add(entry);
                    debug_assert!(
                        old.is_none(),
                        "Impossible: converting Leaf to Tree produced duplicates"
                    );
                }
                return (make_tree.finish(), None);
            }
            NodeContents::TreeWithLeaf {
                len,
                leaf,
                branches,
            } => MakeTree {
                depth,
                len,
                branches,
                leaf: Some(leaf),
            },
            NodeContents::TreeWithoutLeaf { len, branches } => MakeTree {
                depth,
                len,
                branches,
                leaf: None,
            },
        };

        let old = make_tree.add(entry);
        (make_tree.finish(), old)
    }

    pub(crate) fn remove(self, depth: u16, key_bytes: &MerkleKey) -> (Self, Option<(K, V)>) {
        match self {
            NodeContents::Empty => (NodeContents::Empty, None),
            NodeContents::Leaf(mut entries) => {
                for idx in 0..entries.len() {
                    match entries[idx].key_bytes.cmp(key_bytes) {
                        std::cmp::Ordering::Less => (),
                        std::cmp::Ordering::Equal => {
                            let old = entries.remove(idx);
                            return (
                                if entries.is_empty() {
                                    NodeContents::Empty
                                } else {
                                    NodeContents::Leaf(entries)
                                },
                                Some((old.key, old.value)),
                            );
                        }
                        std::cmp::Ordering::Greater => break,
                    }
                }

                (NodeContents::Leaf(entries), None)
            }
            NodeContents::TreeWithLeaf {
                len,
                leaf,
                branches,
            } => remove_tree(depth, key_bytes, len, branches, Some(leaf)),
            NodeContents::TreeWithoutLeaf { len, branches } => {
                remove_tree(depth, key_bytes, len, branches, None)
            }
        }
    }

    fn drain(self, v: &mut Vec<LeafEntry<K, V>>) {
        let branches = match self {
            NodeContents::Empty => return,
            NodeContents::Leaf(mut entries) => {
                v.append(&mut entries);
                return;
            }
            NodeContents::TreeWithLeaf {
                len: _,
                leaf,
                branches,
            } => {
                v.push(leaf);
                branches
            }
            NodeContents::TreeWithoutLeaf { len: _, branches } => branches,
        };
        for branch in branches {
            branch.node.0.into_inner().drain(v);
        }
    }
}

struct MakeTree<K, V> {
    depth: u16,
    len: usize,
    branches: Vec<NodeWithU4<K, V>>,
    leaf: Option<LeafEntry<K, V>>,
}

impl<K, V> MakeTree<K, V> {
    fn add(&mut self, entry: LeafEntry<K, V>) -> Option<LeafEntry<K, V>> {
        let index = match entry.key_bytes.get_index_for_depth(self.depth) {
            None => {
                let old = self.leaf.replace(entry);
                if old.is_none() {
                    self.len += 1;
                }
                return old;
            }
            Some(index) => index,
        };

        let mut dest_idx = self.branches.len();
        for idx in 0..self.branches.len() {
            match self.branches[idx].halfbyte.cmp(&index) {
                // Keep looking
                std::cmp::Ordering::Less => (),
                // It's a match, add it here
                std::cmp::Ordering::Equal => {
                    let branch = self.branches.remove(idx);
                    let (branch, removed) =
                        branch.node.0.into_inner().insert(self.depth + 1, entry);
                    let new_idx = self.branches.len();
                    self.branches.push(NodeWithU4 {
                        halfbyte: index,
                        node: Node(MerkleLockable::new(branch)),
                    });
                    if idx != new_idx {
                        self.branches.swap(idx, new_idx);
                    }
                    if removed.is_none() {
                        self.len += 1;
                    }
                    return removed;
                }
                std::cmp::Ordering::Greater => {
                    dest_idx = idx;
                    break;
                }
            }
        }

        let branch = NodeContents::Leaf(vec![entry]);
        self.branches.insert(
            dest_idx,
            NodeWithU4 {
                halfbyte: index,
                node: Node(MerkleLockable::new(branch)),
            },
        );
        self.len += 1;

        #[cfg(test)]
        {
            assert!(self.branches.is_sorted_by(|x, y| x.halfbyte < y.halfbyte));
        }
        None
    }

    fn finish(self) -> NodeContents<K, V> {
        let Self {
            depth: _,
            len,
            branches,
            leaf,
        } = self;
        match leaf {
            Some(leaf) => NodeContents::TreeWithLeaf {
                len,
                leaf,
                branches,
            },
            None => NodeContents::TreeWithoutLeaf { len, branches },
        }
    }
}

fn remove_tree<K, V>(
    depth: u16,
    key_bytes: &MerkleKey,
    mut len: usize,
    mut branches: Vec<NodeWithU4<K, V>>,
    mut leaf: Option<LeafEntry<K, V>>,
) -> (NodeContents<K, V>, Option<(K, V)>) {
    let mut old = None;
    match key_bytes.get_index_for_depth(depth) {
        None => {
            old = leaf.take().map(|entry| (entry.key, entry.value));
            if old.is_some() {
                len -= 1;
            }
        }
        Some(index) => {
            for idx in 0..branches.len() {
                match index.cmp(&branches[idx].halfbyte) {
                    std::cmp::Ordering::Less => (),
                    std::cmp::Ordering::Equal => {
                        let branch = branches.remove(idx);
                        let (branch, removed) =
                            branch.node.0.into_inner().remove(depth + 1, key_bytes);
                        old = removed;
                        if !branch.is_empty() {
                            let branch = NodeWithU4 {
                                halfbyte: index,
                                node: Node(MerkleLockable::new(branch)),
                            };
                            branches.insert(idx, branch);
                        }
                        if old.is_some() {
                            len -= 1;
                        }
                        break;
                    }
                    std::cmp::Ordering::Greater => break,
                }
            }
        }
    };

    let contents = if len <= 16 {
        debug_assert_eq!(len, 16);
        let mut entries = vec![];
        if let Some(entry) = leaf {
            entries.push(entry);
        }
        for branch in branches {
            branch.node.0.into_inner().drain(&mut entries);
        }
        debug_assert_eq!(entries.len(), len);
        NodeContents::Leaf(entries)
    } else {
        match leaf {
            Some(leaf) => NodeContents::TreeWithLeaf {
                len,
                leaf,
                branches,
            },
            None => NodeContents::TreeWithoutLeaf { len, branches },
        }
    };

    (contents, old)
}

impl<K, V: MerkleSerializeRaw> MerkleSerializeRaw for NodeContents<K, V> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        match self {
            NodeContents::Empty => serialize_leaf::<K, V>(&[], serializer),
            NodeContents::Leaf(entries) => serialize_leaf(entries, serializer),
            NodeContents::TreeWithLeaf {
                len,
                leaf,
                branches,
            } => serialize_tree(*len, Some(leaf), branches, serializer),
            NodeContents::TreeWithoutLeaf { len, branches } => {
                serialize_tree(*len, None, branches, serializer)
            }
        }
    }
}

fn serialize_leaf<K, V: MerkleSerializeRaw>(
    entries: &[LeafEntry<K, V>],
    serializer: &mut MerkleSerializer,
) -> Result<(), MerkleSerialError> {
    serializer.store_byte(42);
    serializer.store_usize(entries.len());
    for entry in entries {
        entry.merkle_serialize_raw(serializer)?;
    }
    Ok(())
}

fn serialize_tree<K, V: MerkleSerializeRaw>(
    len: usize,
    leaf: Option<&LeafEntry<K, V>>,
    branches: &[NodeWithU4<K, V>],
    serializer: &mut MerkleSerializer,
) -> Result<(), MerkleSerialError> {
    serializer.store_byte(43);
    serializer.store_usize(len);
    match leaf {
        Some(leaf) => {
            serializer.store_byte(1);
            leaf.merkle_serialize_raw(serializer)?;
        }
        None => serializer.store_byte(0),
    }

    // Overly complicated because the original serialization format
    // requires it.
    let mut idx = 0;

    for halfbyte in 0..16 {
        if branches.len() > idx && branches[idx].halfbyte == halfbyte {
            serializer.store_by_hash(branches[idx].node.0.as_ref())?;
            idx += 1;
        } else {
            serializer.store_by_hash(&NodeContents::<K, V>::Empty)?;
        }
    }
    Ok(())
}

impl<K: FromMerkleKey, V: MerkleDeserializeRaw> MerkleDeserializeRaw for NodeContents<K, V> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        match deserializer.pop_byte()? {
            42 => deserialize_leaf(deserializer),
            43 => deserialize_tree(deserializer),
            byte => Err(MerkleSerialError::UnexpectedMagicByte { byte }),
        }
    }
}

fn deserialize_leaf<K: FromMerkleKey, V: MerkleDeserializeRaw>(
    deserializer: &mut MerkleDeserializer,
) -> Result<NodeContents<K, V>, MerkleSerialError> {
    let len = deserializer.load_usize()?;
    if len > 16 {
        return Err(MerkleSerialError::LeafContentLimitExceeded {
            limit: 16,
            actual: len,
        });
    }
    let mut values = Vec::new();
    for _ in 0..len {
        values.push(LeafEntry::merkle_deserialize_raw(deserializer)?);
    }

    Ok(if len == 0 {
        NodeContents::Empty
    } else {
        NodeContents::Leaf(values)
    })
}

fn deserialize_tree<K, V>(
    deserializer: &mut MerkleDeserializer,
) -> Result<NodeContents<K, V>, MerkleSerialError> {
    todo!()
}
