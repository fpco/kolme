use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::Hash,
    sync::{Arc, LazyLock, Weak},
};

use parking_lot::RwLock;
use shared::types::Sha256Hash;

use super::MerkleContents;

/// A lookup key for a cached merkle deserialized value.
#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub(super) struct LockKey {
    pub(super) type_id: TypeId,
    pub(super) hash: Sha256Hash,
}

/// The type-erased lock value.
///
/// This is a type-erased version of an Arc<T>.
///
/// Contains the serialized [MerkleContents] as well.
struct LockValue {
    inner: Box<dyn Any + Send + Sync + 'static>,
    contents: Arc<MerkleContents>,
}

/// A newtype wrapper a LockKey that handles dropping.
struct CleanupLockKey(LockKey);

/// An entry in the value cache.
///
/// This includes both the LockValue as well as a Weak reference
/// to the LockKey. Motivation: we want to be able to reuse the same
/// LockKey so that its Drop impl can clear values from the cache.
struct CacheEntry {
    key: Weak<CleanupLockKey>,
    value: LockValue,
}

/// The cache itself
static CACHE: LazyLock<RwLock<Cache>> = LazyLock::new(Default::default);

#[derive(Default)]
struct Cache(HashMap<LockKey, CacheEntry>);

impl Drop for CleanupLockKey {
    fn drop(&mut self) {
        CACHE.write().0.remove(&self.0);
    }
}

/// A locked value, containing the [CleanupLockKey] for keeping the cache alive, the raw value, and the serialized [MerkleContents] version.
pub(crate) struct Locked<T> {
    /// This field is used for drops only, it's never read directly.
    #[allow(dead_code)]
    key: Arc<CleanupLockKey>,
    value: Arc<T>,
    // TODO: for better sharing, would it make sense to have a top-level
    // cache for the MerkleContents?
    contents: Arc<MerkleContents>,
}

impl Cache {
    fn get<T: Send + Sync + 'static>(&self, lock_key: &LockKey) -> Option<Locked<T>> {
        let cache_entry = self.0.get(lock_key)?;
        let key = cache_entry.key.upgrade()?;
        let (value, contents) = cache_entry.value.to_inner()?;
        Some(Locked {
            key,
            value,
            contents,
        })
    }
}

impl<T> Locked<T> {
    pub(super) fn value(&self) -> &Arc<T> {
        &self.value
    }

    pub(super) fn contents(&self) -> &Arc<MerkleContents> {
        &self.contents
    }
}

impl<T: Send + Sync + 'static> Locked<T> {
    pub(super) fn new(lock_key: LockKey, inner: Arc<T>, contents: Arc<MerkleContents>) -> Self {
        if let Some(locked) = CACHE.read().get(&lock_key) {
            return locked;
        }

        let mut guard = CACHE.write();
        if let Some(locked) = guard.get(&lock_key) {
            return locked;
        }

        let cleanup_lock_key = Arc::new(CleanupLockKey(lock_key.clone()));
        let cache_entry = CacheEntry {
            key: Arc::downgrade(&cleanup_lock_key),
            value: LockValue::from_inner(inner.clone(), contents.clone()),
        };
        guard.0.insert(lock_key, cache_entry);
        Locked {
            key: cleanup_lock_key,
            value: inner,
            contents,
        }
    }

    pub(super) fn get(lock_key: LockKey) -> Option<Self> {
        CACHE.read().get(&lock_key)
    }
}

impl LockValue {
    fn from_inner<T: Send + Sync + 'static>(inner: Arc<T>, contents: Arc<MerkleContents>) -> Self {
        LockValue {
            inner: Box::new(inner),
            contents,
        }
    }

    fn to_inner<T: Send + Sync + 'static>(&self) -> Option<(Arc<T>, Arc<MerkleContents>)> {
        Some((self.inner.downcast_ref().cloned()?, self.contents.clone()))
    }
}
