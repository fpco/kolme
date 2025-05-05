use std::{path::Path, sync::Arc};

use fjall::PartitionCreateOptions;
use merkle_map::{MerkleSerialError, MerkleStore, Sha256Hash};

#[derive(Clone)]
pub struct MerkleFjallStore {
    keyspace: fjall::Keyspace,
    handle: fjall::PartitionHandle,
}

pub struct MerkleFjallStoreHelper<'a>(&'a MerkleFjallStore);

impl MerkleFjallStore {
    pub fn new(fjall_dir: impl AsRef<Path>) -> Result<Self, MerkleSerialError> {
        let keyspace = fjall::Config::new(fjall_dir)
            .open()
            .map_err(MerkleSerialError::custom)?;
        let handle = keyspace
            .open_partition("merkle", PartitionCreateOptions::default())
            .map_err(MerkleSerialError::custom)?;
        Ok(Self { keyspace, handle })
    }

    pub fn get_keyspace(&self) -> &fjall::Keyspace {
        &self.keyspace
    }

    /// Return a data structure which we can use for [MerkleStore].
    ///
    /// Reasoning: [MerkleStore] requires mutable/exclusive access to the data structure
    /// it uses. `fjall` itself provides an interface which works fine through
    /// immutable/shared references. By providing this simple wrapper, we can meet
    /// the API requirements of [MerkleStore] without introducing unnecessary
    /// cloning.
    pub fn to_store(&self) -> MerkleFjallStoreHelper {
        MerkleFjallStoreHelper(self)
    }
}

impl MerkleStore for MerkleFjallStoreHelper<'_> {
    async fn load_by_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<Arc<[u8]>>, MerkleSerialError> {
        self.0
            .handle
            .get(hash.as_array())
            .map(|oslice| oslice.map(|slice| Arc::<[u8]>::from(slice.to_vec())))
            .map_err(MerkleSerialError::custom)
    }

    async fn save_by_hash(
        &mut self,
        hash: Sha256Hash,
        payload: &[u8],
    ) -> Result<(), MerkleSerialError> {
        self.0
            .handle
            .insert(hash.as_array(), payload)
            .map_err(MerkleSerialError::custom)
    }

    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        self.0
            .handle
            .contains_key(hash.as_array())
            .map_err(MerkleSerialError::custom)
    }
}
