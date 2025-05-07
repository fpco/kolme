use super::MerkleMap;

#[derive(Clone, Debug, PartialEq)]
pub struct SerializableSlice<'a, T>(pub &'a [T]);

#[derive(Clone, Debug, PartialEq)]
pub struct SerializableMerkleMap<K, V>(pub MerkleMap<K, V>);
