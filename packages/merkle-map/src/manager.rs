use std::sync::LazyLock;

use dashmap::DashMap;
use shared::types::Sha256Hash;

use crate::*;

pub struct MerkleManager<Store> {
    store: Store,
    cache: DashMap<Sha256Hash, Arc<[u8]>>,
}

impl<Store> MerkleManager<Store> {
    pub fn new(store: Store) -> Self {
        MerkleManager {
            store,
            cache: DashMap::new(),
        }
    }
}

pub(crate) fn empty() -> (Sha256Hash, Arc<[u8]>) {
    static EMPTY: LazyLock<(Sha256Hash, Arc<[u8]>)> = LazyLock::new(|| {
        let payload = vec![41].into();
        let hash = Sha256Hash::hash(&payload);
        (hash, payload)
    });
    EMPTY.clone()
}

impl<Store: MerkleWrite> MerkleManager<Store> {
    pub fn save<K, V: ToMerkleBytes>(
        &self,
        map: &mut MerkleMap<K, V>,
    ) -> Result<Sha256Hash, Store::Error> {
        let (hash, payload) = map.lock();
        self.store_recursive(&map.0)?;
        self.store.save_merkle_by_hash(hash, &payload)?;
        self.cache.insert(hash, payload);
        Ok(hash)
    }

    fn store_recursive<K, V: ToMerkleBytes>(&self, node: &Node<K, V>) -> Result<(), Store::Error> {
        match node {
            Node::Empty => {
                let (hash, payload) = empty();
                self.store_if_missing(hash, &payload)?;
            }
            Node::LockedLeaf(leaf) => {
                self.store_if_missing(leaf.hash, &leaf.payload)?;
            }
            Node::UnlockedLeaf(_) => unreachable!(),
            Node::LockedTree(tree) => {
                if self.store_if_missing(tree.hash, &tree.payload)? {
                    for branch in &tree.inner.branches {
                        self.store_recursive(branch)?;
                    }
                }
            }
            Node::UnlockedTree(_) => unreachable!(),
        }
        Ok(())
    }

    /// Returns true if it was missing, false if it was already there.
    fn store_if_missing(&self, hash: Sha256Hash, payload: &[u8]) -> Result<bool, Store::Error> {
        if self.store.contains_hash(hash)? {
            Ok(false)
        } else {
            self.store.save_merkle_by_hash(hash, payload)?;
            Ok(true)
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LoadMerkleMapError<StoreError> {
    #[error(transparent)]
    StoreError {
        #[from]
        source: StoreError,
    },
    #[error("Insufficient input when parsing buffer")]
    InsufficientInput,
    #[error("A usize value would be larger than the machine representation")]
    UsizeOverflow,
    #[error(
        "Unexpected magic byte to distinguish Tree from Leaf, expected 0 or 1, but got {byte}"
    )]
    UnexpectedMagicByte { byte: u8 },
    #[error("Invalid byte at start of tree, expected 0 or 1, but got {byte}")]
    InvalidTreeStart { byte: u8 },
    #[error("Leftover input was unconsumed")]
    TooMuchInput,
    #[error("Serialized content was invalid")]
    InvalidSerializedContent,
    #[error("Hash not found in store: {hash}")]
    HashNotFound { hash: shared::types::Sha256Hash },
}

impl<Store: MerkleRead> MerkleManager<Store> {
    pub fn load<K: FromMerkleBytes, V: FromMerkleBytes>(
        &self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleMap<K, V>>, LoadMerkleMapError<Store::Error>> {
        let payload = match self.cache.get(&hash) {
            Some(payload) => Some(payload.value().clone()),
            None => self.store.load_merkle_by_hash(hash)?,
        };
        match payload {
            None => Ok(None),
            Some(payload) => {
                let node = Node::load(payload, self)?;
                Ok(Some(MerkleMap(node)))
            }
        }
    }
}
