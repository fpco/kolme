use std::{
    any::{Any, TypeId},
    collections::HashMap,
    hash::Hash,
    sync::{Arc, LazyLock, Weak},
};

use get_size2::GetSize;
use parking_lot::RwLock;
use shared::types::Sha256Hash;

/// A lookup key for a cached merkle deserialized value.
#[derive(Hash, PartialEq, Eq, Clone, Debug, GetSize)]
pub(super) struct LockKey {
    #[get_size(size = 16)]
    pub(super) type_id: TypeId,
    pub(super) hash: Sha256Hash,
}

/// The type-erased lock value.
///
/// This is a type-erased version of an Arc<T>.
struct LockValue(Box<dyn Any + Send + Sync + 'static>);

/// A newtype wrapper a LockKey that handles dropping.
#[derive(GetSize)]
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

/// A locked value, containing the [CleanupLockKey] for keeping the cache alive and the raw value.
#[derive(GetSize)]
pub(crate) struct Locked<T> {
    key: Arc<CleanupLockKey>,
    value: Arc<T>,
}

impl Cache {
    fn get<T: Send + Sync + 'static>(&self, lock_key: &LockKey) -> Option<Locked<T>> {
        let cache_entry = self.0.get(lock_key)?;
        let key = cache_entry.key.upgrade()?;
        let value = cache_entry.value.to_inner()?;
        Some(Locked { key, value })
    }
}

impl<T> Locked<T> {
    pub(super) fn hash(&self) -> Sha256Hash {
        self.key.0.hash
    }

    pub(super) fn value(&self) -> &Arc<T> {
        &self.value
    }
}

impl<T: Send + Sync + 'static> Locked<T> {
    pub(super) fn new(lock_key: LockKey, inner: Arc<T>) -> Self {
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
            value: LockValue::from_inner(inner.clone()),
        };
        guard.0.insert(lock_key, cache_entry);
        Locked {
            key: cleanup_lock_key,
            value: inner,
        }
    }

    pub(super) fn get(lock_key: LockKey) -> Option<Self> {
        CACHE.read().get(&lock_key)
    }
}

impl LockValue {
    fn from_inner<T: Send + Sync + 'static>(inner: Arc<T>) -> Self {
        LockValue(Box::new(inner))
    }

    fn to_inner<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.0.downcast_ref().cloned()
    }
}
