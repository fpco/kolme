use std::{fmt::Debug, sync::OnceLock};

use crate::*;

/// Allows a value to be locked with a pre-computed Merkle hash.
pub struct MerkleLockable<T> {
    pub(super) locked: Arc<OnceLock<Sha256Hash>>,
    inner: Arc<T>,
}

impl<T: Debug> Debug for MerkleLockable<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("MerkleLockable").field(&self.inner).finish()
    }
}

impl<T> AsRef<T> for MerkleLockable<T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T: Clone> AsMut<T> for MerkleLockable<T> {
    fn as_mut(&mut self) -> &mut T {
        if self.locked.get().is_some() {
            self.locked = Arc::new(OnceLock::new());
        }
        Arc::make_mut(&mut self.inner)
    }
}

impl<T> MerkleLockable<T> {
    #[cfg(test)]
    pub fn assert_locked_status(&self, expected: bool) {
        assert_eq!(self.locked.get().is_some(), expected);
    }
}

impl<T: MerkleSerializeRaw> MerkleSerializeRaw for MerkleLockable<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        self.inner.merkle_serialize_raw(serializer)
    }

    fn get_merkle_hash_raw(&self) -> Option<Sha256Hash> {
        self.locked.get().copied()
    }

    fn set_merkle_hash_raw(&self, hash: Sha256Hash) {
        self.locked.set(hash).ok();
    }
}

impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for MerkleLockable<T> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        T::merkle_deserialize_raw(deserializer).map(MerkleLockable::new)
    }

    fn load_merkle_by_hash(hash: Sha256Hash) -> Option<Self> {
        todo!()
    }
}

impl<T: PartialOrd> PartialOrd for MerkleLockable<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.inner.partial_cmp(&other.inner)
    }
}

impl<T: Ord> Ord for MerkleLockable<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl<T: PartialEq> PartialEq for MerkleLockable<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq(&other.inner)
    }
}

impl<T: Eq> Eq for MerkleLockable<T> {}

impl<T> Clone for MerkleLockable<T> {
    fn clone(&self) -> Self {
        MerkleLockable {
            locked: self.locked.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl<T: Clone> MerkleLockable<T> {
    pub fn into_inner(self) -> T {
        Arc::unwrap_or_clone(self.inner)
    }
}

impl<T> MerkleLockable<T> {
    pub fn new(inner: T) -> Self {
        MerkleLockable {
            locked: Arc::new(OnceLock::new()),
            inner: Arc::new(inner),
        }
    }
}
