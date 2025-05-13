use std::{
    borrow::Cow,
    ops::{Bound, RangeBounds},
};

use crate::*;

impl<'a, K, V> IntoIterator for &'a Node<K, V> {
    type Item = (&'a K, &'a V);

    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            node: self,
            start: Bound::Unbounded,
            end: Bound::Unbounded,
        }
    }
}

impl<K, V> Node<K, V> {
    pub(crate) fn iter(&self) -> Iter<K, V> {
        self.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a MerkleMap<K, V> {
    type Item = (&'a K, &'a V);

    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

pub struct Iter<'a, K, V> {
    node: &'a Node<K, V>,
    start: Bound<Cow<'a, MerkleKey>>,
    end: Bound<Cow<'a, MerkleKey>>,
}

impl<K, V> Iter<'_, K, V> {
    fn is_complete(&self) -> bool {
        let (start, start_inc) = match &self.start {
            Bound::Included(start) => (start, true),
            Bound::Excluded(start) => (start, false),
            Bound::Unbounded => return false,
        };
        let (end, end_inc) = match &self.end {
            Bound::Included(end) => (end, true),
            Bound::Excluded(end) => (end, false),
            Bound::Unbounded => return false,
        };
        match start.cmp(end) {
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => !start_inc && !end_inc,
            std::cmp::Ordering::Greater => true,
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_complete() {
            return None;
        }
        let (key, included) = match &self.start {
            Bound::Included(start) => (start, true),
            Bound::Excluded(start) => (start, false),
            Bound::Unbounded => (&Cow::Owned(MerkleKey::default()), true),
        };

        let entry = if included {
            if let Some(entry) = self.node.get(0, key) {
                entry
            } else {
                self.node.find_entry_after(0, key)?
            }
        } else {
            self.node.find_entry_after(0, key)?
        };
        self.start = Bound::Excluded(Cow::Borrowed(&entry.key_bytes));
        match &self.end {
            Bound::Included(end) => {
                if end < &Cow::Borrowed(&entry.key_bytes) {
                    return None;
                }
            }
            Bound::Excluded(end) => {
                if end <= &Cow::Borrowed(&entry.key_bytes) {
                    return None;
                }
            }
            Bound::Unbounded => (),
        }
        Some((&entry.key, &entry.value))
    }
}

impl<K, V> DoubleEndedIterator for Iter<'_, K, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.is_complete() {
            return None;
        }
        let (key, entry) = match &self.end {
            Bound::Included(end) => (Some(end), self.node.get(0, end)),
            Bound::Excluded(end) => (Some(end), None),
            Bound::Unbounded => (None, self.node.find_last_entry()),
        };

        let entry = match entry {
            Some(entry) => entry,
            None => {
                let key = key?;

                self.node.find_entry_before(0, key)?
            }
        };

        self.end = Bound::Excluded(Cow::Borrowed(&entry.key_bytes));
        match &self.start {
            Bound::Included(start) => {
                if start > &Cow::Borrowed(&entry.key_bytes) {
                    return None;
                }
            }
            Bound::Excluded(start) => {
                if start >= &Cow::Borrowed(&entry.key_bytes) {
                    return None;
                }
            }
            Bound::Unbounded => (),
        }
        Some((&entry.key, &entry.value))
    }
}

impl<K, V> Node<K, V> {
    fn find_entry_after(&self, depth: u16, key: &MerkleKey) -> Option<&LeafEntry<K, V>> {
        match self {
            Node::Leaf(lockable) => lockable
                .as_ref()
                .values
                .iter()
                .find(|entry| entry.key_bytes > *key),
            Node::Tree(lockable) => {
                let tree = lockable.as_ref();
                if let Some(leaf) = &tree.leaf {
                    if leaf.key_bytes > *key {
                        return Some(leaf);
                    }
                }
                let branch = key.get_index_for_depth(depth).unwrap_or_default();

                // For an exact match, continue checking the key's branches
                if let Some(entry) =
                    tree.branches[usize::from(branch)].find_entry_after(depth + 1, key)
                {
                    return Some(entry);
                }

                // And for all the other branches, simply try and find the first entry
                for branch in tree.branches[usize::from(branch)..].iter().skip(1) {
                    if let Some(entry) = branch.find_first_entry() {
                        return Some(entry);
                    }
                }
                None
            }
        }
    }

    fn find_entry_before(&self, depth: u16, key: &MerkleKey) -> Option<&LeafEntry<K, V>> {
        match self {
            Node::Leaf(lockable) => lockable
                .as_ref()
                .values
                .iter()
                .rev()
                .find(|entry| entry.key_bytes < *key),
            Node::Tree(lockable) => {
                let tree = lockable.as_ref();
                let branch = key.get_index_for_depth(depth).unwrap_or_default();

                // Check the specified branch first and continue finding entries before...
                if let Some(entry) =
                    tree.branches[usize::from(branch)].find_entry_before(depth + 1, key)
                {
                    return Some(entry);
                }

                // Otherwise, take any entry from the previous branches
                for branch in tree.branches[0..usize::from(branch)].iter().rev() {
                    if let Some(entry) = branch.find_last_entry() {
                        return Some(entry);
                    }
                }

                // And if none of the branches have any entries, take our leaf
                if let Some(leaf) = &tree.leaf {
                    if leaf.key_bytes < *key {
                        return Some(leaf);
                    }
                }
                None
            }
        }
    }

    fn find_first_entry(&self) -> Option<&LeafEntry<K, V>> {
        match self {
            Node::Leaf(leaf) => leaf.as_ref().values.first(),
            Node::Tree(tree) => {
                let tree = tree.as_ref();
                if let Some(leaf) = &tree.leaf {
                    return Some(leaf);
                }
                for branch in tree.branches.iter() {
                    if let Some(entry) = branch.find_first_entry() {
                        return Some(entry);
                    }
                }
                None
            }
        }
    }

    fn find_last_entry(&self) -> Option<&LeafEntry<K, V>> {
        match self {
            Node::Leaf(leaf) => leaf.as_ref().values.last(),
            Node::Tree(tree) => {
                let tree = tree.as_ref();
                for branch in tree.branches.iter().rev() {
                    if let Some(entry) = branch.find_last_entry() {
                        return Some(entry);
                    }
                }
                tree.leaf.as_ref()
            }
        }
    }
}

pub struct IntoIter<K, V>(Node<K, V>);

impl<K: Clone, V: Clone> IntoIterator for MerkleMap<K, V> {
    type Item = (K, V);

    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.0)
    }
}

impl<K: Clone, V: Clone> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop_first()
    }
}

impl<K, V> Node<K, V>
where
    K: Clone,
    V: Clone,
{
    fn pop_first(&mut self) -> Option<(K, V)> {
        match self {
            Node::Leaf(leaf) => {
                let leaf = leaf.as_mut();
                if leaf.values.is_empty() {
                    None
                } else {
                    let entry = leaf.values.remove(0);
                    Some((entry.key, entry.value))
                }
            }
            Node::Tree(tree) => {
                let tree = tree.as_mut();
                if let Some(entry) = tree.leaf.take() {
                    return Some((entry.key, entry.value));
                }

                for branch in &mut tree.branches {
                    if let Some(pair) = branch.pop_first() {
                        return Some(pair);
                    }
                }
                None
            }
        }
    }
}

impl<'a, K: ToMerkleKey, V> Node<K, V> {
    pub(crate) fn range<T, R>(&'a self, range: R) -> Iter<'a, K, V>
    where
        T: ToMerkleKey + ?Sized,
        K: Borrow<T>,
        R: RangeBounds<T>,
    {
        Iter {
            node: self,
            start: range.start_bound().map(|x| Cow::Owned(x.to_merkle_key())),
            end: range.end_bound().map(|x| Cow::Owned(x.to_merkle_key())),
        }
    }
}
