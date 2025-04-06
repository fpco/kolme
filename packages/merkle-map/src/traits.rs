use std::sync::Arc;

use dashmap::DashMap;
use shared::types::Sha256Hash;

use crate::MerkleBytes;

pub trait ToMerkleBytes {
    fn to_merkle_bytes(&self) -> MerkleBytes;
}

impl ToMerkleBytes for String {
    fn to_merkle_bytes(&self) -> MerkleBytes {
        MerkleBytes::from_slice(self.as_bytes())
    }
}

impl ToMerkleBytes for str {
    fn to_merkle_bytes(&self) -> MerkleBytes {
        MerkleBytes::from_slice(self.as_bytes())
    }
}

impl ToMerkleBytes for u8 {
    fn to_merkle_bytes(&self) -> MerkleBytes {
        MerkleBytes::from_slice(&[*self])
    }
}
impl ToMerkleBytes for u32 {
    fn to_merkle_bytes(&self) -> MerkleBytes {
        MerkleBytes::from_slice(&self.to_le_bytes())
    }
}

pub trait FromMerkleBytes: Sized {
    fn from_merkle_bytes(bytes: &[u8]) -> Option<Self>;
}

impl FromMerkleBytes for String {
    fn from_merkle_bytes(bytes: &[u8]) -> Option<Self> {
        std::str::from_utf8(bytes).ok().map(String::from)
    }
}

impl FromMerkleBytes for u8 {
    fn from_merkle_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() == 1 {
            Some(bytes[0])
        } else {
            None
        }
    }
}
impl FromMerkleBytes for u32 {
    fn from_merkle_bytes(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(u32::from_le_bytes)
    }
}

pub trait MerkleRead {
    type Error;

    fn load_merkle_by_hash(&self, hash: Sha256Hash) -> Result<Option<Arc<[u8]>>, Self::Error>;
}

pub trait MerkleWrite: MerkleRead {
    fn save_merkle_by_hash(&self, hash: Sha256Hash, payload: &[u8]) -> Result<(), Self::Error>;

    fn contains_hash(&self, hash: Sha256Hash) -> Result<bool, Self::Error>;
}

#[derive(Clone, Default)]
pub struct MerkleMemoryStore(Arc<DashMap<Sha256Hash, Arc<[u8]>>>);

impl MerkleRead for MerkleMemoryStore {
    type Error = std::convert::Infallible;

    fn load_merkle_by_hash(&self, hash: Sha256Hash) -> Result<Option<Arc<[u8]>>, Self::Error> {
        Ok(self.0.get(&hash).map(|x| x.value().clone()))
    }
}

impl MerkleWrite for MerkleMemoryStore {
    fn save_merkle_by_hash(&self, hash: Sha256Hash, payload: &[u8]) -> Result<(), Self::Error> {
        if let Some(value) = self.0.get(&hash) {
            assert_eq!(&**value, payload);
            return Ok(());
        }
        self.0.insert(hash, payload.into());
        Ok(())
    }

    fn contains_hash(&self, hash: Sha256Hash) -> Result<bool, Self::Error> {
        Ok(self.0.contains_key(&hash))
    }
}
