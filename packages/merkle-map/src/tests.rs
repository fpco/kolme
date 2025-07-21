use parameterized::parameterized;
use paste::paste;
use quickcheck::quickcheck;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::Bound,
};

use crate::quickcheck_newtypes::{
    SerializableMerkleMap, SerializableSlice, SerializableSmallVec, SerializableTimestamp,
};

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
                leaf.as_ref().sanity_checks();
            }
            Node::Tree(tree) => {
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

#[tokio::test]
async fn load_should_have_locked_status() {
    let mut tree = MerkleMap::<u8, u8>::new();
    tree.insert(1, 1);
    tree.assert_locked_status(false);
    let manager = MerkleManager::new(TEST_CACHE_SIZE);
    let mut store = MerkleMemoryStore::default();

    let tree_contents = manager.save(&mut store, &tree).await.unwrap();
    tree.assert_locked_status(true);

    let mut same_tree: MerkleMap<u8, u8> =
        manager.load(&mut store, tree_contents.hash).await.unwrap();
    assert_eq!(same_tree, tree);
    same_tree.assert_locked_status(true);

    same_tree.insert(5, 5);
    same_tree.assert_locked_status(false);
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

quickcheck! {
    fn test_store_usize(x: usize) -> bool {
        test_store_usize_inner(x)
    }
}

const TEST_CACHE_SIZE: usize = 1024;

#[tokio::main]
async fn test_store_usize_inner(x: usize) -> bool {
    let manager = MerkleManager::new(TEST_CACHE_SIZE);
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
    let manager = MerkleManager::new(TEST_CACHE_SIZE);
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

        fn merkle_version() -> usize {
            5
        }
    }

    impl MerkleDeserialize for Person {
        fn merkle_deserialize(
            deserializer: &mut MerkleDeserializer,
            version: usize,
        ) -> Result<Self, MerkleSerialError> {
            assert_eq!(version, Person::merkle_version());
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

    let manager = MerkleManager::new(TEST_CACHE_SIZE);
    let contents = manager.serialize(&person).unwrap();
    let person2 = manager
        .deserialize_cached(contents.hash, Arc::new(HashMap::new()))
        .unwrap()
        .unwrap();
    assert_eq!(person, person2);
}

quickcheck! {
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

#[test]
fn rev_iter_sample1() {
    assert!(rev_iter_prop_helper(vec![
        (" ".to_owned(), 0),
        (" \0".to_owned(), 0),
        ("!".to_owned(), 0),
        ("\"".to_owned(), 0),
        ("#".to_owned(), 0),
        ("$".to_owned(), 0),
        (" \u{1}".to_owned(), 0),
        (" \u{2}".to_owned(), 0),
        ("%".to_owned(), 0),
        ("&".to_owned(), 0),
        ("'".to_owned(), 0),
        (" \u{3}".to_owned(), 0),
        ("(".to_owned(), 0),
        (")".to_owned(), 0),
        (" \u{4}".to_owned(), 0),
        ("*".to_owned(), 0),
        (" \u{5}".to_owned(), 0),
        ("\u{1}".to_owned(), 0)
    ]))
}

fn rev_iter_prop_helper(pairs: Vec<(String, u32)>) -> bool {
    let mut mmap = MerkleMap::new();
    let mut bmap = BTreeMap::new();

    for (key, value) in pairs {
        mmap.insert(key.clone(), value);
        bmap.insert(key, value);
    }

    let in_order_actual = mmap.iter().collect::<Vec<_>>();
    let in_order_expected = bmap.iter().collect::<Vec<_>>();
    assert_eq!(in_order_actual, in_order_expected);

    let rev_actual = mmap.iter().rev().collect::<Vec<_>>();
    assert_eq!(
        rev_actual,
        in_order_actual.into_iter().rev().collect::<Vec<_>>()
    );
    let rev_expected = bmap.iter().rev().collect::<Vec<_>>();
    assert_eq!(
        rev_expected,
        in_order_expected.into_iter().rev().collect::<Vec<_>>()
    );
    assert_eq!(rev_actual, rev_expected);

    true
}

quickcheck! {
    fn rev_iter_prop(pairs: Vec<(String, u32)>) -> bool {
        rev_iter_prop_helper(pairs)
    }
}

#[test]
fn rev_iter_empty_string() {
    let pairs = vec![
        ("\0", 0),
        ("\u{1}", 0),
        ("\u{2}", 0),
        ("\u{3}", 0),
        ("\u{4}", 0),
        ("\u{5}", 0),
        ("\u{6}", 0),
        ("\u{7}", 0),
        ("\u{8}", 0),
        ("", 0),
        ("\t", 0),
        ("\n", 0),
        ("\u{b}", 0),
        ("\u{c}", 0),
        ("\r", 0),
        ("\u{e}", 0),
        ("\u{f}", 0),
    ];
    let mut mmap = MerkleMap::new();
    let mut bmap = BTreeMap::new();

    for (key, value) in pairs {
        mmap.insert(key.to_owned(), value);
        bmap.insert(key.to_owned(), value);
    }

    let in_order_actual = mmap.iter().collect::<Vec<_>>();
    let in_order_expected = bmap.iter().collect::<Vec<_>>();
    assert_eq!(in_order_actual, in_order_expected);

    let rev_actual = mmap.iter().rev().collect::<Vec<_>>();
    let rev_expected = bmap.iter().rev().collect::<Vec<_>>();
    assert_eq!(rev_actual, rev_expected);
}

#[test]
fn range_asc() {
    let mut map = MerkleMap::new();
    for c in 'A'..='Z' {
        map.insert(format!("{c}"), ());
    }
    let in_order_actual = map
        .range("K".to_owned().."W".to_owned())
        .map(|(x, ())| x.clone())
        .collect::<Vec<_>>();
    let in_order_expected = ('K'..='V').map(|c| format!("{c}")).collect::<Vec<_>>();
    assert_eq!(in_order_actual, in_order_expected);
}

#[test]
fn range_desc() {
    let mut map = MerkleMap::new();
    for c in 'A'..='Z' {
        map.insert(format!("{c}"), ());
    }
    let in_order_actual = map
        .range("K".to_owned().."W".to_owned())
        .rev()
        .map(|(x, ())| x.clone())
        .collect::<Vec<_>>();
    let in_order_expected = ('K'..='V')
        .rev()
        .map(|c| format!("{c}"))
        .collect::<Vec<_>>();
    assert_eq!(in_order_actual, in_order_expected);
}

#[test]
fn range_exclude_empty() {
    let mut mmap = MerkleMap::new();
    mmap.insert("".to_owned(), ());
    assert_eq!(mmap.range(.."".to_owned()).next_back(), None);
    assert_eq!(
        mmap.range("".to_owned()..).next_back(),
        Some((&"".to_owned(), &()))
    );
    assert_eq!(
        mmap.range("".to_owned()..).next(),
        Some((&"".to_owned(), &()))
    );
    assert_eq!(mmap.range(.."".to_owned()).next(), None);
}

fn range_helper(
    pairs: Vec<(String, u32)>,
    asc: bool,
    start: Bound<String>,
    end: Bound<String>,
) -> bool {
    let mut mmap = MerkleMap::new();
    let mut bmap = BTreeMap::new();

    fn in_range(key: &str, start: &Bound<String>, stop: &Bound<String>) -> bool {
        (match start {
            Bound::Included(start) => start.as_str() <= key,
            Bound::Excluded(start) => start.as_str() < key,
            Bound::Unbounded => true,
        }) && (match stop {
            Bound::Included(stop) => stop.as_str() >= key,
            Bound::Excluded(stop) => stop.as_str() > key,
            Bound::Unbounded => true,
        })
    }
    for (key, value) in &pairs {
        if in_range(key, &start, &end) {
            bmap.insert(key.clone(), value);
        }
        mmap.insert(key.clone(), value);
    }

    let expected = if asc {
        bmap.iter().collect::<Vec<_>>()
    } else {
        bmap.iter().rev().collect()
    };
    let mmap = mmap.range((start.clone(), end.clone()));
    let actual = if asc {
        mmap.collect::<Vec<_>>()
    } else {
        mmap.rev().collect()
    };
    let pairs_bytes = pairs
        .iter()
        .map(|(k, v)| (k.as_bytes(), v))
        .collect::<Vec<_>>();
    let start_bytes = start.as_ref().map(|x| x.as_bytes());
    let end_bytes = end.as_ref().map(|x| x.as_bytes());
    assert_eq!(expected, actual, "range_helper failed on input:\nPairs: {pairs_bytes:?}\nAsc: {asc}\nStart: {start_bytes:?}\nEnd: {end_bytes:?}\nExpected: {expected:?}\nActual:   {actual:?}");
    true
}

quickcheck! {
    fn range(pairs: Vec<(String, u32)>, asc: bool, start: Bound<String>, stop: Bound<String>) -> bool {
        range_helper(pairs, asc, start, stop)
    }
}

#[test]
fn range_sample() {
    let pairs: &[(Vec<u8>, u64)] = &[
        (
            vec![
                36u8, 104, 59, 194, 145, 64, 63, 120, 226, 128, 179, 70, 241, 175, 157, 185, 243,
                191, 191, 189, 226, 128, 181, 8, 70, 94, 92, 197, 137, 120, 194, 157, 90, 89, 220,
                143, 226, 128, 175, 238, 143, 147, 123, 243, 175, 163, 191, 95, 238, 128, 129, 238,
                189, 157, 10, 50, 20, 244, 143, 191, 189, 226, 128, 164, 225, 160, 142, 194, 141,
                50, 232, 177, 133, 99, 63, 14, 45, 15, 26, 37, 225, 154, 128, 77, 194, 131, 194,
                128, 13, 42, 35, 226, 129, 160, 49, 227, 159, 173, 42, 242, 131, 151, 159, 52,
            ],
            1585135052u64,
        ),
        (vec![42, 111, 96], 2501899558),
        (
            vec![
                61, 194, 144, 53, 226, 129, 154, 239, 132, 158, 240, 184, 166, 169, 240, 169, 169,
                135, 234, 153, 144, 48,
            ],
            542627149,
        ),
        (
            vec![
                38, 194, 167, 54, 194, 151, 37, 231, 170, 136, 9, 232, 144, 179, 59, 46, 64, 125,
                10, 35, 25, 62, 194, 161, 228, 170, 130, 194, 174, 64, 194, 138, 70, 58, 20, 58,
                48, 12, 117, 124, 57, 242, 155, 156, 161, 44, 194, 141, 65, 88, 5, 54, 64, 233,
                161, 145, 115, 234, 169, 164, 35, 25, 17, 226, 128, 167, 45, 40, 51, 13, 21, 241,
                163, 146, 131, 107, 194, 156, 105, 194, 169, 124, 97, 37, 36, 225, 154, 128, 194,
                169, 110, 242, 144, 161, 141, 44, 239, 191, 183, 83, 194, 171, 241, 145, 186, 170,
                226, 128, 170, 108, 80, 63,
            ],
            3955258220,
        ),
        (
            vec![
                33, 48, 239, 149, 168, 194, 148, 194, 166, 56, 226, 128, 187, 194, 172, 194, 143,
                243, 160, 128, 160, 230, 175, 189, 124, 194, 157, 56, 194, 136, 194, 149, 108, 45,
                121, 50, 61, 194, 160,
            ],
            503095582,
        ),
        (
            vec![
                42, 2, 235, 155, 187, 91, 60, 22, 229, 188, 156, 55, 194, 145, 62, 238, 174, 173,
                243, 148, 171, 185, 194, 170, 39, 194, 169, 194, 149, 194, 130, 36, 203, 161, 48,
                10, 48, 52, 241, 170, 183, 166, 107, 194, 171, 84, 48, 42, 194, 134, 38, 60, 229,
                143, 138, 94, 226, 128, 143, 194, 157, 243, 133, 186, 133, 82, 232, 166, 152, 58,
                75, 14, 126, 78, 27, 35, 37, 229, 190, 140, 226, 129, 135, 243, 176, 128, 128, 66,
                124, 63, 233, 139, 144, 62, 194, 167, 194, 141, 42, 5, 49, 61, 242, 158, 143, 183,
                194, 144, 116, 104, 225, 148, 180, 20, 194, 160, 81, 39, 194, 141, 123, 35, 194,
                132, 242, 157, 182, 134, 70, 194, 171, 57, 105, 27, 46, 32, 28, 229, 185, 147, 194,
                157, 33, 36, 121, 50, 239, 191, 186, 239, 147, 131, 48, 64,
            ],
            119277783,
        ),
        (
            vec![
                32, 194, 168, 194, 146, 40, 79, 244, 128, 128, 128, 235, 165, 159, 46, 241, 180,
                160, 154, 58, 13, 53, 10, 32, 194, 137, 48, 232, 170, 146, 236, 154, 129, 1, 194,
                163, 9, 194, 135, 237, 143, 157, 22, 226, 128, 181, 48, 53, 117, 39, 73, 40, 194,
                144, 85,
            ],
            786827649,
        ),
        (
            vec![36, 59, 227, 178, 143, 35, 55, 56, 96, 194, 152],
            915111728,
        ),
        (
            vec![
                40, 84, 194, 165, 240, 176, 150, 187, 194, 137, 98, 75, 58, 194, 130, 31, 194, 136,
                226, 129, 132, 226, 129, 167, 226, 149, 184, 226, 129, 171, 229, 160, 149, 63, 103,
                94, 243, 151, 133, 137, 95, 94, 47, 64, 71, 242, 136, 181, 136, 82, 231, 147, 166,
                124, 48, 42, 103, 92, 98, 91, 64, 110, 194, 163, 226, 129, 146, 38, 231, 177, 143,
                115, 33, 35, 241, 139, 174, 166, 72, 194, 173, 91, 114, 44, 226, 128, 169, 194,
                132, 238, 128, 128, 43, 92, 24, 73, 32, 31, 10, 121, 52, 226, 129, 142, 75, 49,
                194, 158, 226, 143, 191, 224, 188, 148, 93, 235, 144, 183, 20, 65, 194, 134, 243,
                160, 128, 129, 21, 42,
            ],
            1098383665,
        ),
        (
            vec![
                36, 233, 187, 156, 10, 194, 133, 77, 194, 167, 2, 49, 194, 152, 32, 229, 150, 170,
                6, 46, 125, 51, 60, 10, 226, 128, 148,
            ],
            1524366075,
        ),
        (
            vec![
                43, 59, 101, 194, 157, 194, 171, 46, 243, 174, 142, 150, 125, 124, 90, 37, 194,
                164, 62, 51, 108, 243, 143, 167, 142, 35, 10, 243, 160, 128, 160, 226, 129, 169,
                194, 174, 63, 231, 142, 141, 66, 41, 125, 64, 2, 226, 128, 167, 194, 159, 61, 226,
                128, 188, 38, 41, 34, 56, 113, 56, 96, 61, 225, 151, 133, 243, 132, 135, 134, 194,
                128, 38, 60, 194, 142, 226, 128, 188, 32, 124, 194, 147, 116, 76, 63, 243, 160,
                128, 129, 70, 40, 52, 126, 32, 84, 62, 234, 190, 132, 23, 228, 188, 175, 91, 32,
                51, 231, 172, 146, 194, 160, 111, 194, 165, 194, 142, 240, 145, 130, 189, 44, 14,
                43, 239, 161, 142, 242, 128, 179, 185, 25, 45, 239, 191, 179,
            ],
            925018081,
        ),
        (
            vec![
                33, 49, 49, 226, 128, 189, 119, 194, 141, 47, 87, 57, 226, 128, 174,
            ],
            1545562557,
        ),
        (
            vec![
                39, 194, 154, 44, 78, 234, 131, 147, 194, 134, 95, 227, 157, 135, 77, 231, 169,
                142, 115, 103, 226, 128, 128, 57, 36, 194, 142,
            ],
            3139916951,
        ),
        (
            vec![
                44, 61, 232, 134, 143, 83, 32, 226, 129, 162, 194, 171, 226, 128, 182, 111, 231,
                130, 170, 194, 154, 226, 129, 148, 116, 238, 139, 139, 240, 156, 129, 181, 59, 97,
                194, 156, 216, 131, 239, 191, 190, 235, 160, 178, 226, 129, 164, 194, 131, 194,
                169, 224, 182, 185, 93, 194, 169, 233, 184, 141, 43, 225, 132, 165, 227, 187, 186,
                45, 194, 135, 92, 71, 240, 175, 190, 165, 33, 66, 232, 185, 151, 44, 13, 34, 28,
                58, 64, 5, 194, 158, 194, 148,
            ],
            2494442083,
        ),
        (
            vec![
                43, 57, 237, 137, 189, 42, 117, 58, 124, 234, 180, 131, 79, 13, 226, 129, 165, 194,
                151, 242, 187, 161, 170, 226, 128, 141, 67, 240, 153, 128, 181, 233, 162, 139, 38,
                46, 46, 19, 25, 194, 132, 240, 175, 148, 174, 94, 102, 48, 220, 143, 43, 219, 157,
                96, 238, 141, 144, 226, 128, 150, 194, 154, 87, 234, 172, 131, 23, 231, 180, 184,
                92, 194, 138, 61, 93, 0, 102, 60,
            ],
            0,
        ),
        (
            vec![
                41, 58, 40, 42, 48, 239, 191, 186, 194, 133, 82, 194, 174, 54, 99, 36, 47, 49, 24,
                65, 227, 128, 128, 0, 38, 194, 136, 194, 166, 56, 93, 240, 175, 137, 139, 194, 134,
                241, 173, 137, 185, 94, 73, 5, 244, 128, 128, 128, 92, 30, 12,
            ],
            3127087097,
        ),
        (
            vec![
                48, 61, 124, 116, 56, 45, 235, 135, 140, 194, 160, 82, 194, 139, 194, 151, 228,
                152, 136, 60, 50, 91, 229, 133, 147, 59, 103, 239, 191, 186, 46, 105, 226, 128,
                128, 116, 2, 10, 44, 62, 226, 128, 167, 13, 24, 60, 244, 128, 128, 128, 240, 166,
                140, 170, 58, 50, 45, 125, 194, 128,
            ],
            3581867985,
        ),
        (
            vec![
                43, 71, 52, 22, 108, 226, 129, 134, 24, 226, 129, 149, 64, 225, 170, 170, 28, 43,
                120, 194, 130,
            ],
            2427361170,
        ),
        (
            vec![
                39, 63, 194, 138, 239, 191, 188, 226, 128, 143, 194, 131, 194, 139, 100, 237, 139,
                145, 244, 143, 191, 191, 194, 150, 59, 242, 133, 175, 134, 18, 32, 226, 128, 175,
                230, 140, 154, 64, 110, 7, 228, 135, 129, 54, 80, 45, 240, 184, 142, 151, 123, 194,
                146, 40, 216, 130, 70, 194, 143, 97, 118, 225, 154, 128, 15, 243, 169, 176, 155,
                32,
            ],
            602441893,
        ),
    ];

    let mut mmap = MerkleMap::new();
    let mut bmap = BTreeMap::new();

    for (key, value) in pairs {
        let key = std::str::from_utf8(key).unwrap();
        bmap.insert(key.to_owned(), value);
        mmap.insert(key.to_owned(), value);
    }

    assert_eq!(pairs.len(), mmap.len());
    assert_eq!(pairs.len(), bmap.len());

    {
        let mut mmap2 = mmap.iter().rev();
        let mut mmap = mmap.iter();
        let mut bmap = bmap.iter();

        for i in (0..pairs.len()).rev() {
            let m2 = mmap2.next().unwrap().0;
            let m = mmap.next_back().unwrap().0;
            assert_eq!(m, m2);
            let b = bmap.next_back().unwrap().0;
            println!("{i}:\n    {:?}\n    {:?}", m.as_bytes(), b.as_bytes());
            assert_eq!(m.as_bytes(), b.as_bytes(), "Failed at index {i}");
        }

        assert_eq!(mmap.next_back(), None);
        assert_eq!(bmap.next_back(), None);
    }

    let expected = bmap.iter().rev().collect::<Vec<_>>();
    let expected2 = mmap.iter().collect::<Vec<_>>();
    let expected2 = expected2.into_iter().rev().collect::<Vec<_>>();
    assert_eq!(expected, expected2);
    let actual = mmap.iter().rev().collect::<Vec<_>>();
    assert_eq!(pairs.len(), actual.len());
    assert_eq!(expected[0], actual[0]);
    assert_eq!(expected[expected.len() - 1], actual[actual.len() - 1]);
    for i in 0..expected.len().min(actual.len()) {
        assert_eq!(
            expected[i], actual[i],
            "Mismatch at {i}:\nExpected: {}\nActual:   {}",
            expected[i].0, actual[i].0
        );
    }
    assert_eq!(expected.len(), actual.len());
    assert_eq!(expected, actual);
}

macro_rules! serializing_idempotency_for_type {
    ($value_type: ty, $test_name: ident) => {
        quickcheck! {
            fn $test_name(value: $value_type) -> quickcheck::TestResult {
                let manager = MerkleManager::new(TEST_CACHE_SIZE);
                let serialized = manager.serialize(&value).unwrap();
                let deserialized = manager
                    .deserialize::<$value_type>(serialized.hash, serialized.payload.clone())
                    .unwrap();

                quickcheck::TestResult::from_bool(value == deserialized)
            }
        }

        paste! {
            // tests for Option<T>
            quickcheck!{
                fn [<$test_name _option>] (value: Option<$value_type>) -> quickcheck::TestResult {
                    let manager = MerkleManager::new(TEST_CACHE_SIZE);
                    let serialized = manager.serialize(&value).unwrap();
                    let deserialized = manager
                        .deserialize::<Option<$value_type>>(serialized.hash, serialized.payload.clone())
                        .unwrap();

                    quickcheck::TestResult::from_bool(value == deserialized)
                }
            }

            // tests for [T]
            quickcheck!{
                fn [<$test_name _slice>] (value: SerializableSlice<'static, $value_type>) -> quickcheck::TestResult {
                    let manager = MerkleManager::new(TEST_CACHE_SIZE);
                    let serialized = manager.serialize(value.0).unwrap();
                    let deserialized = manager
                        .deserialize::<Vec<$value_type>>(serialized.hash, serialized.payload.clone())
                        .unwrap();

                    quickcheck::TestResult::from_bool(value.0 == deserialized)
                }
            }

            // tests for BTreeSet<T>
            quickcheck!{
                fn [<$test_name _btreeset>] (value: BTreeSet<$value_type>) -> quickcheck::TestResult {
                    let manager = MerkleManager::new(TEST_CACHE_SIZE);
                    let serialized = manager.serialize(&value).unwrap();
                    let deserialized = manager
                        .deserialize::<BTreeSet<$value_type>>(serialized.hash, serialized.payload.clone())
                        .unwrap();

                    quickcheck::TestResult::from_bool(value == deserialized)
                }
            }
        }
    };
}

serializing_idempotency_for_type!(String, serializing_is_idempotent_for_string);
serializing_idempotency_for_type!(usize, serializing_is_idempotent_for_usize);
serializing_idempotency_for_type!(u8, serializing_is_idempotent_for_u8);
serializing_idempotency_for_type!(u32, serializing_is_idempotent_for_u32);
serializing_idempotency_for_type!(u64, serializing_is_idempotent_for_u64);
serializing_idempotency_for_type!(u128, serializing_is_idempotent_for_u128);

quickcheck! {
    fn serializing_is_idempotent_for_btreemap_string_u64(value: BTreeMap<String, u64>) -> quickcheck::TestResult {
        let manager = MerkleManager::new(TEST_CACHE_SIZE);
        let serialized = manager.serialize(&value).unwrap();
        let deserialized = manager
            .deserialize::<BTreeMap<String, u64>>(serialized.hash, serialized.payload.clone())
            .unwrap();

        quickcheck::TestResult::from_bool(value == deserialized)
    }
    fn serializing_is_idempotent_for_merklemap_string_u64 (value: SerializableMerkleMap<String, u64>) -> quickcheck::TestResult {
        let manager = MerkleManager::new(TEST_CACHE_SIZE);
        let serialized = manager.serialize(&value.0).unwrap();
        let deserialized = manager
            .deserialize::<MerkleMap<String, u64>>(serialized.hash, serialized.payload.clone())
            .unwrap();

        quickcheck::TestResult::from_bool(value == SerializableMerkleMap(deserialized))
    }
    fn serializing_is_idempotent_for_timestamp (value: SerializableTimestamp) -> quickcheck::TestResult {
        let manager = MerkleManager::new(TEST_CACHE_SIZE);
        let serialized = manager.serialize(&value.0).unwrap();
        let deserialized = SerializableTimestamp(manager
            .deserialize(serialized.hash, serialized.payload.clone())
            .unwrap());

        quickcheck::TestResult::from_bool(value == deserialized)
    }

    fn serializing_is_idempotent_for_smallvec_u64_4 (value: SerializableSmallVec<[u64;4]>) -> quickcheck::TestResult {
        let manager = MerkleManager::new(TEST_CACHE_SIZE);
        let serialized = manager.serialize(&value.0).unwrap();
        let deserialized = SerializableSmallVec(manager
            .deserialize(serialized.hash, serialized.payload.clone())
            .unwrap());

        quickcheck::TestResult::from_bool(value == deserialized)
    }

    fn serializing_is_idempotent_for_2_tuple_of_primitives(value: (u64, String))-> quickcheck::TestResult {
        let manager = MerkleManager::new(TEST_CACHE_SIZE);
        let serialized = manager.serialize(&value).unwrap();
        let deserialized: (u64, String) = manager
            .deserialize(serialized.hash, serialized.payload.clone())
            .unwrap();

        quickcheck::TestResult::from_bool(value == deserialized)
    }

}
