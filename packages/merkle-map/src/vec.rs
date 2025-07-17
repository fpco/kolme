use crate::*;

/// Provides a [Vec]-like API, but built on top of a [MerkleMap].
///
/// This supports up to 2^32 entries.
#[derive(Clone)]
pub struct MerkleVec<T>(MerkleMap<u32, T>);

impl<T: MerkleSerializeRaw> MerkleSerializeRaw for MerkleVec<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for MerkleVec<T> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load().map(Self)
    }
}

impl<T> Default for MerkleVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> MerkleVec<T> {
    pub fn new() -> Self {
        Self(MerkleMap::new())
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter().map(|(_, v)| v)
    }
}

impl<T: Clone> MerkleVec<T> {
    pub fn push(&mut self, t: T) {
        let k = self.0.iter().next_back().map_or(0, |(k, _)| *k + 1);
        self.0.insert(k, t);
    }
}

impl<T: Clone> IntoIterator for MerkleVec<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> IntoIter<T> {
        IntoIter(self.0.into_iter())
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for MerkleVec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

pub struct IntoIter<T: Clone>(<MerkleMap<u32, T> as IntoIterator>::IntoIter);

impl<T: Clone> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanity() {
        let mut v = MerkleVec::new();
        v.push(5);
        v.push(4);
        v.push(3);
        assert_eq!(v.into_iter().collect::<Vec<_>>(), vec![5, 4, 3]);
    }
}
