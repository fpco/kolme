use crate::*;

use std::fmt::Debug;

impl<K, V> Node<K, V> {
    #[cfg(test)]
    pub(crate) fn sanity_checks(&self) {
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
}

impl<K, V> LeafContents<K, V> {
    pub(crate) fn sanity_checks(&self) {
        assert!(!self.values.is_empty());
        assert!(self.values.len() <= 16);
        let mut prev = None;
        for entry in &self.values {
            if let Some(prev) = prev {
                assert!(
                    prev < &entry.key_bytes,
                    "prev ({prev:?}) >= entry.key_bytes ({:?})",
                    entry.key_bytes
                );
            }
            prev = Some(&entry.key_bytes);
        }
    }
}

impl<K, V> std::fmt::Debug for LeafContents<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "[")?;
        for value in &self.values {
            write!(f, "{:?},", value.key_bytes)?;
        }
        Ok(())
    }
}

impl<K, V> TreeContents<K, V> {
    fn sanity_checks(&self) {
        assert!(self.branches.iter().any(|branch| !branch.is_empty()));
        let expected = self.branches.iter().map(|node| node.len()).sum::<usize>()
            + if self.leaf.is_some() { 1 } else { 0 };
        assert_eq!(self.len(), expected);
        self.branches.iter().for_each(Node::sanity_checks);
    }
}
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

#[test]
fn duplicates() {
    let mut tree = MerkleTree::<u32, u32>::new();
    for i in 0..100u32 {
        assert_eq!(tree.len(), i as usize);
        assert_eq!(tree.insert(i, i), None);
        assert_eq!(tree.len(), (i + 1) as usize);
        assert_eq!(tree.insert(i, i), Some((i, i)));
        assert_eq!(tree.len(), (i + 1) as usize);
    }
}

#[test]
fn overlapping_keys() {
    let mut tree = MerkleTree::<String, u32>::new();
    tree.insert("abc".to_owned(), 1);
    tree.insert("ab".to_owned(), 2);
    tree.insert("abcd".to_owned(), 3);

    fn test_tree(tree: &MerkleTree<String, u32>) {
        assert_eq!(tree.get("ab"), Some(&2));
        assert_eq!(tree.get("abcd"), Some(&3));
        assert_eq!(tree.get("abc"), Some(&1));
        assert_eq!(tree.get("abcde"), None);
        assert_eq!(tree.get(""), None);
    }

    test_tree(&tree);

    // Fun way to run this test: keep expanding the size of the tree and confirm
    // that we never lose any entries
    for i in 0..100 {
        assert_eq!(tree.len(), i as usize + 3);
        tree.insert(i.to_string(), i);
        test_tree(&tree);
    }

    // And now go the other way too
    for i in 0..100 {
        assert_eq!(tree.len(), 100 - i as usize + 3);
        assert_eq!(tree.remove(&i.to_string()), Some((i.to_string(), i)));
        test_tree(&tree);
    }
}

#[test]
fn just_a() {
    let mut tree = MerkleTree::new();

    fn make_str(count: usize) -> String {
        let mut s = String::with_capacity(count);
        for _ in 0..count {
            s.push('a');
        }
        s
    }

    for i in 0..100 {
        assert_eq!(tree.len(), i);
        tree.insert(make_str(i), i);
        assert_eq!(tree.len(), i + 1);
        assert_eq!(tree.insert(make_str(i), i), Some((make_str(i), i)));
    }

    for i in (0..100).rev() {
        assert_eq!(tree.len(), i + 1);
        assert_eq!(tree.remove(&make_str(i)), Some((make_str(i), i)));
        assert_eq!(tree.len(), i);
        assert_eq!(tree.remove(&make_str(i)), None);
        assert_eq!(tree.len(), i);
    }
}
