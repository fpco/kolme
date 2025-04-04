use std::{borrow::Borrow, sync::Arc};

use shared::types::Sha256Hash;

pub struct MerkleTree<K, V>(Node<K, V>);

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct MerkleKeyBytes(Vec<u8>);
impl MerkleKeyBytes {
    fn get_index_for_depth(&self, depth: u16) -> Option<u8> {
        // First get the byte in question...
        let byte = *self.0.get(usize::from(depth / 2))?;
        // Then get either the high or low bits, depending on whether our depth is even or odd
        Some(if depth % 2 == 0 { byte & 0x0F } else { todo!() })
    }
}

pub trait MerkleKey {
    fn to_bytes(&self) -> MerkleKeyBytes;
}

#[derive(Clone)]
struct LeafEntry<K, V> {
    key_bytes: MerkleKeyBytes,
    key: K,
    value: V,
}

struct Locked<T>(Arc<LockedInner<T>>);

struct LockedInner<T> {
    hash: Sha256Hash,
    inner: T,
}

impl<T> Clone for Locked<T> {
    fn clone(&self) -> Self {
        Locked(self.0.clone())
    }
}

impl<T: Clone> Locked<T> {
    fn into_inner(self) -> T {
        match Arc::try_unwrap(self.0) {
            Ok(x) => x.inner,
            Err(x) => T::clone(&x.inner),
        }
    }
}

#[derive(Clone)]
enum Node<K, V> {
    Empty,
    LockedLeaf(Locked<LeafContents<K, V>>),
    UnlockedLeaf(LeafContents<K, V>),
    LockedTree(Locked<TreeContents<K, V>>),
    UnlockedTree(Box<TreeContents<K, V>>),
    // FIXME
    // Lazy {
    //     hash: Sha256Hash,
    // },
}

impl<K, V> Node<K, V> {
    fn is_empty(&self) -> bool {
        match self {
            Node::Empty => true,
            Node::LockedLeaf(_)
            | Node::UnlockedLeaf(_)
            | Node::LockedTree(_)
            | Node::UnlockedTree(_) => false,
        }
    }

    fn len(&self) -> usize {
        match self {
            Node::Empty => 0,
            Node::LockedLeaf(leaf) => leaf.0.inner.len(),
            Node::UnlockedLeaf(leaf) => leaf.len(),
            Node::LockedTree(tree) => tree.0.inner.len(),
            Node::UnlockedTree(tree) => tree.len(),
        }
    }

    #[cfg(test)]
    fn sanity_checks(&self) {
        match self {
            Node::Empty => (),
            Node::LockedLeaf(leaf) => {
                // FIXME validate hashes
                leaf.0.inner.sanity_checks();
            }
            Node::UnlockedLeaf(leaf) => {
                leaf.sanity_checks();
            }
            Node::LockedTree(tree) => {
                // FIXME validate hashes
                tree.0.inner.sanity_checks();
            }
            Node::UnlockedTree(tree) => {
                tree.sanity_checks();
            }
        }
    }

    fn update_cursor(&self, cursor: &mut Cursor) -> Option<(&K, &V)> {
        match self {
            Node::Empty => None,
            Node::LockedLeaf(_) => todo!(),
            Node::UnlockedLeaf(_) => todo!(),
            Node::LockedTree(_) => todo!(),
            Node::UnlockedTree(_) => todo!(),
        }
    }
}

impl<K: MerkleKey + Clone, V: Clone> Node<K, V> {
    fn unlock(self) -> UnlockedNode<K, V> {
        match self {
            Node::Empty => UnlockedNode::Leaf(LeafContents::default()),
            Node::LockedLeaf(leaf) => UnlockedNode::Leaf(leaf.into_inner()),
            Node::UnlockedLeaf(leaf) => UnlockedNode::Leaf(leaf),
            Node::LockedTree(tree) => UnlockedNode::Tree(tree.into_inner()),
            Node::UnlockedTree(tree) => UnlockedNode::Tree(*tree),
        }
    }

    fn get<Q>(&self, depth: u16, key_bytes: MerkleKeyBytes, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        match self {
            Node::Empty => None,
            Node::LockedLeaf(leaf) => leaf.0.inner.get(key_bytes, key),
            Node::UnlockedLeaf(leaf) => leaf.get(key_bytes, key),
            Node::LockedTree(tree) => tree.0.inner.get(depth, key_bytes, key),
            Node::UnlockedTree(tree) => tree.get(depth, key_bytes, key),
        }
    }
}
impl<K, V> Default for Node<K, V> {
    fn default() -> Self {
        Node::Empty
    }
}

impl<K: MerkleKey + Clone, V: Clone> From<TreeContents<K, V>> for LeafContents<K, V> {
    fn from(tree: TreeContents<K, V>) -> Self {
        assert!(tree.len() <= 16);
        let mut leaf = LeafContents { values: vec![] };
        tree.drain_entries_to(&mut leaf.values);
        leaf
    }
}

enum UnlockedNode<K, V> {
    Leaf(LeafContents<K, V>),
    Tree(TreeContents<K, V>),
}
impl<K: MerkleKey + Clone, V: Clone> From<UnlockedNode<K, V>> for Node<K, V> {
    fn from(node: UnlockedNode<K, V>) -> Self {
        match node {
            UnlockedNode::Leaf(leaf) => {
                if leaf.is_empty() {
                    Node::Empty
                } else {
                    Node::UnlockedLeaf(leaf)
                }
            }
            UnlockedNode::Tree(tree) => {
                let count = tree.len();
                if count == 0 {
                    Node::Empty
                } else if count <= 16 {
                    Node::UnlockedLeaf(tree.into())
                } else {
                    Node::UnlockedTree(Box::new(tree))
                }
            }
        }
    }
}
impl<K: MerkleKey + Clone, V: Clone> UnlockedNode<K, V> {
    fn insert(self, depth: u16, entry: LeafEntry<K, V>) -> (UnlockedNode<K, V>, Option<(K, V)>) {
        match self {
            UnlockedNode::Leaf(leaf) => leaf.insert(depth, entry),
            UnlockedNode::Tree(mut tree) => {
                let v = tree.insert(depth, entry);
                (UnlockedNode::Tree(tree), v)
            }
        }
    }

    fn remove<Q>(
        self,
        depth: u16,
        key_bytes: MerkleKeyBytes,
        key: &Q,
    ) -> (UnlockedNode<K, V>, Option<(K, V)>)
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        match self {
            UnlockedNode::Leaf(mut leaf) => {
                let v = leaf.remove(key_bytes, key);
                (UnlockedNode::Leaf(leaf), v)
            }
            UnlockedNode::Tree(tree) => tree.remove(depth, key_bytes, key),
        }
    }
}

#[derive(Clone)]
struct LeafContents<K, V> {
    /// Invariant: must be sored by key_bytes
    values: Vec<LeafEntry<K, V>>, // FIXME switch to an array
}

#[cfg(test)]
impl<K, V> std::fmt::Debug for LeafContents<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "[")?;
        for value in &self.values {
            write!(f, "{:?},", value.key_bytes)?;
        }
        Ok(())
    }
}

impl<K, V> LeafContents<K, V> {
    fn sort(&mut self) {
        self.values.sort_by(|x, y| x.key_bytes.cmp(&y.key_bytes));
    }
    fn insert(
        mut self,
        depth: u16,
        mut entry: LeafEntry<K, V>,
    ) -> (UnlockedNode<K, V>, Option<(K, V)>)
    where
        K: MerkleKey + Clone,
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

    fn get<Q>(&self, key_bytes: MerkleKeyBytes, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: MerkleKey + ?Sized,
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

    fn remove<Q>(&mut self, key_bytes: MerkleKeyBytes, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
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

    fn find_first(&self) -> Option<&K> {
        self.values.first().map(|x| &x.key)
    }

    #[cfg(test)]
    fn sanity_checks(&self) {
        assert!(!self.values.is_empty());
        assert!(self.values.len() <= 16);
        let mut prev = None;
        for entry in &self.values {
            if let Some(prev) = prev {
                assert!(prev < &entry.key_bytes);
            }
            prev = Some(&entry.key_bytes);
        }
    }

    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn len(&self) -> usize {
        self.values.len()
    }

    fn drain_entries_to(mut self, entries: &mut Vec<LeafEntry<K, V>>) {
        entries.append(&mut self.values);
    }
}

impl<K, V> Default for LeafContents<K, V> {
    fn default() -> Self {
        Self { values: vec![] }
    }
}

#[derive(Clone)]
struct TreeContents<K, V> {
    len: usize,
    branches: [Node<K, V>; 16],
}
impl<K, V> TreeContents<K, V> {
    fn new() -> Self {
        Self {
            len: 0,
            branches: std::array::from_fn(|_| Node::Empty),
        }
    }

    fn find_first(&self) -> Option<&K> {
        todo!()
    }

    #[cfg(test)]
    fn sanity_checks(&self) {
        assert!(self.branches.iter().any(|branch| !branch.is_empty()));
        let expected = self.branches.iter().map(|node| node.len()).sum::<usize>();
        assert_eq!(self.len(), expected);
        self.branches.iter().for_each(Node::sanity_checks);
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl<K: MerkleKey + Clone, V: Clone> TreeContents<K, V> {
    fn insert(&mut self, depth: u16, entry: LeafEntry<K, V>) -> Option<(K, V)> {
        let index = entry
            .key_bytes
            .get_index_for_depth(depth)
            .expect("Impossible: TreeContents::insert without sufficient bytes");
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

    fn get<Q>(&self, depth: u16, key_bytes: MerkleKeyBytes, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        let index = key_bytes
            .get_index_for_depth(depth)
            .expect("Impossible: TreeContents::get without sufficient bytes");
        let index = usize::from(index);
        self.branches[index].get(depth + 1, key_bytes, key)
    }

    fn remove<Q>(
        mut self,
        depth: u16,
        key_bytes: MerkleKeyBytes,
        key: &Q,
    ) -> (UnlockedNode<K, V>, Option<(K, V)>)
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        let index = key_bytes
            .get_index_for_depth(depth)
            .expect("Impossible: TreeContents::remove without sufficient bytes");
        let index = usize::from(index);
        // FIXME check if we need to go back to a leaf because we have few enough nodes
        let branch = std::mem::take(&mut self.branches[index]).unlock();
        let (branch, v) = branch.remove(depth + 1, key_bytes, key);
        self.branches[index] = branch.into();
        if v.is_some() {
            self.len -= 1;
        }
        (UnlockedNode::Tree(self), v)
    }

    fn drain_entries_to(mut self, entries: &mut Vec<LeafEntry<K, V>>) {
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

    fn update_cursor(&self, cursor: &mut Cursor) -> Option<(&K, &V)> {
        self.0.update_cursor(cursor)
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
        self.0.get(0, key.to_bytes(), key)
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: MerkleKey + ?Sized,
    {
        self.sanity_checks();
        let node = std::mem::take(&mut self.0).unlock();
        let (node, v) = node.remove(0, key.to_bytes(), key);
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

impl MerkleKey for String {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes(self.clone().into_bytes())
    }
}

impl MerkleKey for str {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes(self.as_bytes().to_vec())
    }
}

impl MerkleKey for u8 {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes(vec![*self])
    }
}
impl MerkleKey for u32 {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes(self.to_le_bytes().to_vec())
    }
}

pub struct Iter<'a, K, V> {
    tree: &'a MerkleTree<K, V>,
    cursor: Cursor,
}

#[derive(Default)]
struct Cursor(Vec<u8>);

impl<'a, K, V> IntoIterator for &'a MerkleTree<K, V> {
    type Item = (&'a K, &'a V);

    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            tree: self,
            cursor: Cursor::default(),
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        self.tree.update_cursor(&mut self.cursor)
    }
}

pub struct IntoIter<K, V>(MerkleTree<K, V>);

impl<K: MerkleKey + Clone, V: Clone> IntoIterator for MerkleTree<K, V> {
    type Item = (K, V);

    type IntoIter = IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self)
    }
}

impl<K: MerkleKey + Clone, V: Clone> Iterator for IntoIter<K, V> {
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.pop_first()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use super::*;

    impl<K, V> Debug for MerkleTree<K, V> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }

    impl<K, V> Debug for Node<K, V> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match self {
                Node::Empty => write!(f, "Empty"),
                Node::LockedLeaf(leaf) => {
                    write!(f, "LockedLeaf({}, {:?})", leaf.0.hash, leaf.0.inner)
                }
                Node::UnlockedLeaf(leaf) => write!(f, "UnlockedLeaf({leaf:?})"),
                Node::LockedTree(tree) => {
                    write!(f, "LockedTree({}, {:?})", tree.0.hash, tree.0.inner)
                }
                Node::UnlockedTree(tree) => write!(f, "UnlockedTree({tree:?})"),
            }
        }
    }
    impl<K, V> Debug for UnlockedNode<K, V> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match self {
                UnlockedNode::Leaf(leaf) => {
                    write!(f, "Leaf({:?})", leaf)
                }
                UnlockedNode::Tree(tree) => {
                    write!(f, "Tree({:?})", tree)
                }
            }
        }
    }
    impl<K, V> Debug for TreeContents<K, V> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "[")?;
            for branch in &self.branches {
                write!(f, "{branch:?}")?;
            }
            write!(f, "]")
        }
    }

    #[test]
    fn insert_get() {
        let mut tree = MerkleTree::<String, String>::new();
        assert_eq!(tree.get("key1"), None);
        tree.insert("key1".to_owned(), "value1".to_owned());
        assert_eq!(tree.get("key1").map(|x| x.as_str()), Some("value1"));
    }

    #[test]
    fn many_inserts() {
        let mut tree = MerkleTree::<u8, u8>::new();
        for i in 0..100 {
            tree.insert(i, i * 2);
        }

        for i in 0..100 {
            assert_eq!(tree.get(&i).copied(), Some(i * 2));
        }
    }

    #[test]
    fn remove() {
        let mut tree = MerkleTree::<u32, u32>::new();
        tree.insert(5, 12);
        assert_eq!(tree.get(&5).copied(), Some(12));
        assert_eq!(tree.remove(&5), Some((5, 12)));
        assert_eq!(tree.get(&5).copied(), None);
    }

    #[test]
    fn many_removes() {
        let mut tree = MerkleTree::<u32, u32>::new();
        for i in 0..100 {
            tree.insert(i, i * 2);
        }

        for i in 0..100 {
            assert_eq!(tree.remove(&i), Some((i, i * 2)));
        }

        assert!(tree.is_empty());
    }

    #[test]
    fn pop_first() {
        let mut tree = MerkleTree::<String, u32>::new();
        tree.insert("def".to_owned(), 42);
        tree.insert("abc".to_owned(), 43);
        assert_eq!(tree.pop_first(), Some(("abc".to_owned(), 43)));
        assert_eq!(tree.pop_first(), Some(("def".to_owned(), 42)));
        assert_eq!(tree.pop_first(), None);
    }

    #[test]
    fn iterate() {
        let mut tree = MerkleTree::<u32, u32>::new();
        for i in 0..100 {
            tree.insert(i, i * 2);
        }

        let expected = (0..100).map(|x| (x, x * 2)).collect::<Vec<_>>();
        let actual = tree.iter().map(|(x, y)| (*x, *y)).collect::<Vec<_>>();
        assert_eq!(expected, actual);
        let actual = tree.into_iter().collect::<Vec<_>>();
        assert_eq!(expected, actual);
    }
}
