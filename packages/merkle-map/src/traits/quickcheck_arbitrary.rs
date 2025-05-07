use crate::quickcheck_newtypes::SerializableMerkleMap;
use crate::types::MerkleMap;
use quickcheck::Arbitrary;
use std::collections::BTreeMap;

use super::{FromMerkleKey, ToMerkleKey};

impl<K: Arbitrary + ToMerkleKey + FromMerkleKey + Ord, V: Arbitrary> Arbitrary
    for SerializableMerkleMap<K, V>
{
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let tree_map = <BTreeMap<K, V>>::arbitrary(g);
        let mmap: MerkleMap<K, V> = MerkleMap::from_iter(tree_map);
        Self(mmap)
    }
}
