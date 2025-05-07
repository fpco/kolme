use crate::quickcheck_newtypes::{SerializableMerkleMap, SerializableSlice};
use crate::types::MerkleMap;
use quickcheck::Arbitrary;
use std::collections::BTreeMap;

use super::{FromMerkleKey, ToMerkleKey};

impl<T: Arbitrary> Arbitrary for SerializableSlice<'static, T> {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let vectorized = <Vec<T>>::arbitrary(g);
        Self(vectorized.leak())
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
}
