use std::sync::OnceLock;

use shared::types::Sha256Hash;

use crate::*;

pub(crate) struct Lockable<T> {
    locked: Arc<OnceLock<(Sha256Hash, Arc<[u8]>)>>,
    inner: Arc<T>,
}

impl<T> Lockable<T> {
    pub(crate) fn as_ref(&self) -> &Arc<T> {
        &self.inner
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

impl<T: CanLock> CanLock for Lockable<T> {
    fn lock(&self) -> Result<(Sha256Hash, Arc<[u8]>), MerkleSerialError> {
        if let Some(pair) = self.locked.get() {
            return Ok(pair.clone());
        }
        let pair = self.inner.lock()?;
        self.locked.set(pair.clone()).unwrap();
        Ok(pair)
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

    pub(crate) fn new_locked(hash: Sha256Hash, payload: Arc<[u8]>, inner: T) -> Self {
        let locked = OnceLock::new();
        locked.set((hash, payload)).unwrap();
        Lockable {
            locked: Arc::new(locked),
            inner: Arc::new(inner),
        }
    }
}
