use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Weak},
};

use parking_lot::RwLock;
use shared::types::Sha256Hash;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CachedBytes(Arc<CachedBytesInner>);

#[derive(PartialEq, Eq, Debug)]
struct CachedBytesInner {
    hash: Sha256Hash,
    bytes: Arc<[u8]>,
}

impl AsRef<[u8]> for CachedBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0.bytes
    }
}

impl CachedBytes {
    pub fn hash(&self) -> Sha256Hash {
        self.0.hash
    }

    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.0.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.bytes().is_empty()
    }

    pub fn len(&self) -> usize {
        self.bytes().len()
    }

    pub fn new_bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        let bytes = bytes.into();
        let hash = Sha256Hash::hash(&bytes);
        Self::new_hash(hash, bytes)
    }

    /// Invariant: the hash must match the bytes.
    ///
    /// This is only checked in debug assertions for efficiency.
    /// If you provide mismatched data, things will break!
    pub fn new_hash(hash: Sha256Hash, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self(CachedBytesInner::new(hash, bytes))
    }
}

static CACHED_BYTES: LazyLock<RwLock<HashMap<Sha256Hash, Weak<CachedBytesInner>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

impl CachedBytesInner {
    fn new(hash: Sha256Hash, bytes: impl Into<Arc<[u8]>>) -> Arc<Self> {
        if let Some(inner) = CACHED_BYTES.read().get(&hash).and_then(Weak::upgrade) {
            return inner;
        }

        let val = Arc::new(Self {
            hash,
            bytes: bytes.into(),
        });
        debug_assert_eq!(val.hash, Sha256Hash::hash(&val.bytes));
        let mut guard = CACHED_BYTES.write();

        // Check for a data race
        if let Some(inner) = guard.get(&hash).and_then(Weak::upgrade) {
            return inner;
        }

        // No race, insert the value into the cache and return it.
        guard.insert(hash, Arc::downgrade(&val));
        val
    }
}

impl Drop for CachedBytesInner {
    fn drop(&mut self) {
        CACHED_BYTES.write().remove(&self.hash);
    }
}
