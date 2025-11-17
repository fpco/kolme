use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Weak},
};

use parking_lot::RwLock;
use shared::types::Sha256Hash;

use crate::{MerkleContents, MerkleMap, MerkleSerializeRaw, MerkleSerializer};

pub(crate) struct ContentsCacheKey(Sha256Hash);

struct ContentsCacheValue {
    contents: Arc<MerkleContents>,
    key: Weak<ContentsCacheKey>,
}

impl Drop for ContentsCacheKey {
    fn drop(&mut self) {
        CONTENTS_CACHE.write().0.remove(&self.0);
    }
}

/// And a cache of just the contents. See call site in load_merkle_contents.
static CONTENTS_CACHE: LazyLock<RwLock<ContentsCache>> =
    LazyLock::new(|| RwLock::new(new_contents_cache()));

/// Instantiate an empty contents cache containing just the empty MerkleMap.
fn new_contents_cache() -> ContentsCache {
    let mut m = HashMap::new();

    // Fill in an empty MerkleMap to avoid cache misses.
    {
        let mm = MerkleMap::<u8, u8>::new();
        let mut serializer = MerkleSerializer::new();
        match mm.merkle_serialize_raw(&mut serializer) {
            Ok(()) => {
                let contents = serializer.finish();
                let hash = contents.hash();
                let key = Arc::new(ContentsCacheKey(hash));
                let weak = Arc::downgrade(&key);
                m.insert(
                    hash,
                    ContentsCacheValue {
                        contents: Arc::new(contents),
                        key: weak,
                    },
                );

                // We never want this one cleared from the cache, so leak it.
                std::mem::forget(key);
            }
            Err(e) => {
                debug_assert!(
                    false,
                    "new_contents_cache: unexpected error serializing empty MerkleMap: {e}"
                )
            }
        }
    }
    ContentsCache(m)
}

#[derive(Default)]
struct ContentsCache(HashMap<Sha256Hash, ContentsCacheValue>);

pub(crate) fn get_cached_contents(hash: &Sha256Hash) -> Option<Arc<MerkleContents>> {
    CONTENTS_CACHE
        .read()
        .0
        .get(hash)
        .map(|value| value.contents.clone())
}

pub(crate) fn set_cached_contents(contents: Arc<MerkleContents>) -> Arc<ContentsCacheKey> {
    let mut guard = CONTENTS_CACHE.write();
    if let Some(value) = guard.0.get(&contents.hash()) {
        debug_assert_eq!(contents, value.contents);
        return value
            .key
            .upgrade()
            .unwrap_or_else(|| Arc::new(ContentsCacheKey(contents.hash())));
    }

    let key = Arc::new(ContentsCacheKey(contents.hash()));
    let weak = Arc::downgrade(&key);
    guard.0.insert(
        contents.hash(),
        ContentsCacheValue {
            contents,
            key: weak,
        },
    );
    key
}
