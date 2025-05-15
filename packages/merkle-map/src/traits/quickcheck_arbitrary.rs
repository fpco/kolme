use crate::quickcheck_newtypes::{SerializableMerkleMap, SerializableSlice, SerializableTimestamp};
use crate::types::MerkleMap;
use jiff::Timestamp;
use quickcheck::Arbitrary;
use std::collections::BTreeMap;

use super::{FromMerkleKey, ToMerkleKey};

impl<T: Arbitrary> Arbitrary for SerializableSlice<'static, T> {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let vectorized = <Vec<T>>::arbitrary(g);
        Self(vectorized.leak())
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let as_vector: Vec<T> = self.0.to_vec();

        Box::new(as_vector.shrink().map(|shrunk_vec| Self(shrunk_vec.leak())))
    }
}

impl<K: Arbitrary + ToMerkleKey + FromMerkleKey + Ord, V: Arbitrary> Arbitrary
    for SerializableMerkleMap<K, V>
{
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let tree_map = <BTreeMap<K, V>>::arbitrary(g);
        let mmap: MerkleMap<K, V> = MerkleMap::from_iter(tree_map);
        Self(mmap)
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let as_btreemap: BTreeMap<K, V> = BTreeMap::from_iter(
            self.0
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        Box::new(
            as_btreemap
                .shrink()
                .map(|shrunk_btreemap| Self(MerkleMap::from_iter(shrunk_btreemap))),
        )
    }
}

impl Arbitrary for SerializableTimestamp {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let random_ts = <i64>::arbitrary(g);
        let normalized = if random_ts < 0 {
            random_ts % Timestamp::MIN.as_millisecond()
        } else {
            random_ts % Timestamp::MAX.as_millisecond()
        };
        Self(Timestamp::from_millisecond(normalized).unwrap())
    }
}
