use std::{fmt::Debug, sync::OnceLock};

use crate::*;

/// Allows a value to be locked with a pre-computed Merkle hash.
pub struct MerkleLockable<T> {
    locked: Arc<OnceLock<Arc<MerkleContents>>>,
    inner: Arc<T>,
}

impl<T: Debug> Debug for MerkleLockable<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.inner.fmt(f)
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

impl<T: MerkleSerialize> MerkleSerialize for MerkleLockable<T> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        self.inner.merkle_serialize(serializer)
    }

    fn get_merkle_contents(&self) -> Option<Arc<MerkleContents>> {
        self.locked.get().cloned()
    }

    fn set_merkle_contents(&self, contents: &Arc<MerkleContents>) {
        self.locked.set(contents.clone()).ok();
    }
}

impl<T: MerkleDeserialize> MerkleDeserialize for MerkleLockable<T> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        T::merkle_deserialize(deserializer).map(MerkleLockable::new)
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
