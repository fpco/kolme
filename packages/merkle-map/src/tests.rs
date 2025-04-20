use parameterized::parameterized;
use std::collections::BTreeMap;

use crate::*;

impl<K, V> MerkleMap<K, V> {
    pub fn assert_locked_status(&self, expected: bool) {
        self.0.assert_locked_status(expected);
    }
}

impl<K, V> Node<K, V> {
    pub fn assert_locked_status(&self, expected: bool) {
        match self {
            Node::Leaf(leaf) => leaf.assert_locked_status(expected),
            Node::Tree(tree) => {
                tree.assert_locked_status(expected);
                for branch in &tree.as_ref().branches {
                    branch.assert_locked_status(expected);
                }
            }
        }
    }
}

impl<K, V> Node<K, V> {
    #[cfg(test)]
    pub(crate) fn sanity_checks(&self) {
        match self {
            Node::Leaf(leaf) => {
                // FIXME validate hashes
                leaf.as_ref().sanity_checks();
            }
            Node::Tree(tree) => {
                // FIXME validate hashes
                tree.as_ref().sanity_checks();
            }
        }
    }
}

impl<K, V> LeafContents<K, V> {
    pub(crate) fn sanity_checks(&self) {
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

impl<K, V> TreeContents<K, V> {
    fn sanity_checks(&self) {
        assert!(self.branches.iter().any(|branch| !branch.is_empty()));
        let expected = self.branches.iter().map(|node| node.len()).sum::<usize>()
            + if self.leaf.is_some() { 1 } else { 0 };
        assert_eq!(self.len(), expected);
        self.branches.iter().for_each(Node::sanity_checks);
    }
}

#[test]
fn insert_get() {
    let mut tree = MerkleMap::<String, String>::new();
    assert_eq!(tree.get("key1"), None);
    tree.insert("key1".to_owned(), "value1".to_owned());
    assert_eq!(tree.get("key1").map(|x| x.as_str()), Some("value1"));
}

#[test]
fn many_inserts() {
    let mut tree = MerkleMap::<u8, u8>::new();
    for i in 0..100 {
        tree.insert(i, i * 2);
    }

    for i in 0..100 {
        assert_eq!(tree.get(&i).copied(), Some(i * 2));
    }
}

#[test]
fn remove() {
    let mut tree = MerkleMap::<u32, u32>::new();
    tree.insert(5, 12);
    assert_eq!(tree.get(&5).copied(), Some(12));
    assert_eq!(tree.remove(&5), Some((5, 12)));
    assert_eq!(tree.get(&5).copied(), None);
}

#[test]
fn many_removes() {
    let mut tree = MerkleMap::<u32, u32>::new();
    for i in 0..100 {
        tree.insert(i, i * 2);
    }

    for i in 0..100 {
        assert_eq!(tree.remove(&i), Some((i, i * 2)));
    }

    assert!(tree.is_empty());
}

#[parameterized(size = { 17, 100 })]
fn iterate(size: u32) {
    let mut tree = MerkleMap::<u32, u32>::new();
    for i in 0..size {
        tree.insert(i, i * 2);
    }

    let expected = (0..size).map(|x| (x, x * 2)).collect::<Vec<_>>();
    let actual = tree.iter().map(|(x, y)| (*x, *y)).collect::<Vec<_>>();
    assert_eq!(expected, actual);
    let actual = tree.into_iter().collect::<Vec<_>>();
    assert_eq!(expected, actual);
}

#[test]
fn duplicates() {
    let mut tree = MerkleMap::<u32, u32>::new();
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
    let mut tree = MerkleMap::<String, u32>::new();
    tree.insert("abc".to_owned(), 1);
    tree.insert("ab".to_owned(), 2);
    tree.insert("abcd".to_owned(), 3);

    fn test_tree(tree: &MerkleMap<String, u32>) {
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
    let mut tree = MerkleMap::new();

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

quickcheck::quickcheck! {
    fn test_store_usize(x: usize) -> bool {
        test_store_usize_inner(x)
    }
}

#[tokio::main]
async fn test_store_usize_inner(x: usize) -> bool {
    let manager = MerkleManager::default();
    let contents = manager.serialize(&x).unwrap();
    let y = manager
        .deserialize::<usize>(contents.hash, contents.payload.clone())
        .unwrap();
    assert_eq!(x, y);
    true
}

async fn memory_manager_helper(size: u32) {
    let mut m = MerkleMap::new();
    for i in 0..size {
        m.insert(i, i * 2);
    }

    m.assert_locked_status(false);

    let mut store = MerkleMemoryStore::default();
    let manager = MerkleManager::default();
    let contents = manager.save(&mut store, &m).await.unwrap();

    m.assert_locked_status(true);

    let m2 = manager.load(&mut store, contents.hash).await.unwrap();

    m.assert_locked_status(true);

    assert_eq!(m, m2);
}

#[parameterized(size = { 0, 1, 2, 10, 16, 17, 1000 })]
#[parameterized_macro(tokio::test)]
async fn memory_manager(size: u32) {
    memory_manager_helper(size).await
}

fn store_load_helper(name: String, age: u32, inventory: BTreeMap<String, u32>) {
    #[derive(Debug, PartialEq, Eq)]
    struct Person {
        name: String,
        age: u32,
        inventory: BTreeMap<String, u32>,
    }

    impl MerkleSerialize for Person {
        fn merkle_serialize(
            &self,
            serializer: &mut MerkleSerializer,
        ) -> Result<(), MerkleSerialError> {
            serializer.store(&self.name)?;
            serializer.store(&self.age)?;
            serializer.store(&self.inventory)?;
            Ok(())
        }
    }

    impl MerkleDeserialize for Person {
        fn merkle_deserialize(
            deserializer: &mut MerkleDeserializer,
        ) -> Result<Self, MerkleSerialError> {
            Ok(Self {
                name: deserializer.load()?,
                age: deserializer.load()?,
                inventory: deserializer.load()?,
            })
        }
    }

    let person = Person {
        name,
        age,
        inventory,
    };

    let manager = MerkleManager::default();
    let contents = manager.serialize(&person).unwrap();
    let person2 = manager.deserialize_cached(contents.hash).unwrap().unwrap();
    assert_eq!(person, person2);
}

quickcheck::quickcheck! {
    fn store_load(name:String,age:u32, inventory:BTreeMap<String,u32>) -> bool {
        store_load_helper(name,age,inventory);
        true
    }
}

#[test]
fn rev_iter() {
    let mut map = MerkleMap::new();
    for c in 'A'..='Z' {
        map.insert(format!("{c}"), ());
    }
    let in_order_actual = map.iter().map(|(x, ())| x.clone()).collect::<Vec<_>>();
    let in_order_expected = ('A'..='Z').map(|c| format!("{c}")).collect::<Vec<_>>();
    assert_eq!(in_order_actual, in_order_expected);
    let rev_actual = map
        .iter()
        .rev()
        .map(|(x, ())| x.clone())
        .collect::<Vec<_>>();
    let rev_expected = ('A'..='Z')
        .rev()
        .map(|c| format!("{c}"))
        .collect::<Vec<_>>();
    assert_eq!(rev_actual, rev_expected);
}

fn rev_iter_prop_helper(pairs: Vec<(String, u32)>) -> bool {
    let mut mmap = MerkleMap::new();
    let mut bmap = BTreeMap::new();

    for (key, value) in pairs {
        mmap.insert(key.clone(), value);
        bmap.insert(key, value);
    }

    let in_order_actual = mmap.iter().map(|(x, y)| (x.clone(), y)).collect::<Vec<_>>();
    let in_order_expected = bmap.iter().map(|(x, y)| (x.clone(), y)).collect::<Vec<_>>();
    assert_eq!(in_order_actual, in_order_expected);

    let rev_actual = mmap.iter().map(|(x, y)| (x.clone(), y)).collect::<Vec<_>>();
    let rev_expected = bmap.iter().map(|(x, y)| (x.clone(), y)).collect::<Vec<_>>();
    assert_eq!(rev_actual, rev_expected);

    true
}

quickcheck::quickcheck! {
    fn rev_iter_prop(pairs: Vec<(String, u32)>) -> bool {
        rev_iter_prop_helper(pairs)
    }
}
