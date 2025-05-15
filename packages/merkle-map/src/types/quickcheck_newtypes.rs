use jiff::Timestamp;
use smallvec::{Array, SmallVec};

use crate::{MerkleDeserialize, MerkleSerialize};

use super::MerkleMap;

#[derive(Clone, Debug, PartialEq)]
pub struct SerializableSlice<'a, T>(pub &'a [T]);

#[derive(Clone, Debug, PartialEq)]
pub struct SerializableMerkleMap<K, V>(pub MerkleMap<K, V>);

#[derive(Clone, Debug, PartialEq)]
pub struct SerializableTimestamp(pub Timestamp);

#[derive(Clone, Debug, PartialEq)]
pub struct SerializableSmallVec<
    A: Array<Item: MerkleSerialize + MerkleDeserialize + Clone + PartialEq + std::fmt::Debug>,
>(pub SmallVec<A>);
