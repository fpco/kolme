use std::sync::OnceLock;

use crate::*;

pub(crate) struct Lockable<T> {
    locked: Arc<OnceLock<Arc<MerkleContents>>>,
    inner: Arc<T>,
}

impl<T> Lockable<T> {
    pub(crate) fn as_ref(&self) -> &Arc<T> {
        &self.inner
    }

    #[cfg(test)]
    pub fn assert_locked_status(&self, expected: bool) {
        assert_eq!(self.locked.get().is_some(), expected);
    }
}

impl<T> Lockable<T>
where
    T: Clone,
{
    /// This also wipes out the existing locked value.
    pub(crate) fn as_mut(&mut self) -> &mut T {
        if self.locked.get().is_some() {
            self.locked = Arc::new(OnceLock::new());
        }
        Arc::make_mut(&mut self.inner)
    }
}

impl<T: MerkleSerialize> MerkleSerialize for Lockable<T> {
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

impl<T: PartialOrd> PartialOrd for Lockable<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.inner.partial_cmp(&other.inner)
    }
}

impl<T: Ord> Ord for Lockable<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.inner.cmp(&other.inner)
    }
}

impl<T: PartialEq> PartialEq for Lockable<T> {
    fn eq(&self, other: &Self) -> bool {
        self.inner.eq(&other.inner)
    }
}

impl<T: Eq> Eq for Lockable<T> {}

impl<T> Clone for Lockable<T> {
    fn clone(&self) -> Self {
        Lockable {
            locked: self.locked.clone(),
            inner: self.inner.clone(),
        }
    }
}

impl<T: Clone> Lockable<T> {
    pub(crate) fn into_inner(self) -> T {
        Arc::unwrap_or_clone(self.inner)
    }
}

impl<T> Lockable<T> {
    pub(crate) fn new_unlocked(inner: T) -> Self {
        Lockable {
            locked: Arc::new(OnceLock::new()),
            inner: Arc::new(inner),
        }
    }
}
