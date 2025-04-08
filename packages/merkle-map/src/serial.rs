//! Helper types and functions for buffer reading and writing.

use std::sync::Arc;

use shared::types::Sha256Hash;

use crate::{MerkleDeserializer, MerkleManager, MerkleSerializer, MerkleStore};

/// Errors that can occur during serialization of data.
#[derive(thiserror::Error, Debug)]
pub enum MerkleSerialError {
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
    #[error(transparent)]
    Custom(Box<dyn std::error::Error + Send + Sync>),
}

impl MerkleSerialError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(Box::new(e))
    }
}

/// A concrete implementation of a [MerkleSerializer].
pub struct MerkleSerializerImpl<Store> {
    pub(crate) buff: Vec<u8>,
    pub(crate) manager: MerkleManager<Store>,
}

impl<Store: MerkleStore> MerkleSerializer for MerkleSerializerImpl<Store> {
    type Store = Store;

    fn get_manager(&self) -> &MerkleManager<Store> {
        &self.manager
    }

    fn store_byte(&mut self, byte: u8) {
        self.buff.push(byte);
    }

    fn store_raw_bytes(&mut self, bytes: &[u8]) {
        self.buff.extend_from_slice(bytes);
    }

    async fn finish(self) -> Result<(Sha256Hash, Arc<[u8]>), MerkleSerialError> {
        let buff = Arc::<[u8]>::from(self.buff);
        let hash = Sha256Hash::hash(&buff);
        self.manager.save_merkle_by_hash(hash, buff.clone()).await?;
        Ok((hash, buff))
    }

    fn new_serializer(&self) -> Self {
        self.manager.new_serializer()
    }
}

/// A concrete implementation of a [MerkleDeserializer].
pub struct MerkleDeserializerImpl<'a, Store> {
    pub(crate) buff: &'a [u8],
    pub(crate) pos: usize,
    // TODO we'll probably need this so we can add a helper method to MerkleDeserializer
    #[allow(dead_code)]
    pub(crate) manager: MerkleManager<Store>,
}

impl<'a, Store> MerkleDeserializer for MerkleDeserializerImpl<'a, Store> {
    fn pop_byte(&mut self) -> Result<u8, MerkleSerialError> {
        let byte = *self
            .buff
            .get(self.pos)
            .ok_or(MerkleSerialError::InsufficientInput)?;
        self.pos += 1;
        Ok(byte)
    }

    fn load_raw_bytes(&mut self, len: usize) -> Result<&'a [u8], MerkleSerialError> {
        let end = self.pos + len;
        if end > self.buff.len() {
            Err(MerkleSerialError::InsufficientInput)
        } else {
            let slice = &self.buff[self.pos..end];
            self.pos = end;
            Ok(slice)
        }
    }

    fn finish(self) -> Result<(), MerkleSerialError> {
        if self.buff.len() == self.pos {
            Ok(())
        } else {
            Err(MerkleSerialError::TooMuchInput)
        }
    }
}

// FIXME

//     pub(crate) fn load_hash<StoreError>(
//         &mut self,
//     ) -> Result<Sha256Hash, LoadMerkleMapError<StoreError>> {
//         if self.pos + 32 <= self.buff.len() {
//             let hash =
//                 Sha256Hash::from_array(self.buff[self.pos..self.pos + 32].try_into().unwrap());
//             self.pos += 32;
//             Ok(hash)
//         } else {
//             Err(LoadMerkleMapError::InsufficientInput)
//         }
//     }

#[cfg(test)]
mod tests {
    use crate::MerkleMemoryStore;

    use super::*;

    quickcheck::quickcheck! {
        fn test_store_usize(x: usize) -> bool {
            test_store_usize_inner(x)
        }
    }

    #[tokio::main]
    async fn test_store_usize_inner(x: usize) -> bool {
        let store = MerkleMemoryStore::default();
        let manager = MerkleManager::new(store);
        let mut serializer = manager.new_serializer();
        serializer.store_usize(x);
        let (_hash, buff) = serializer.finish().await.unwrap();
        let mut deserializer = manager.new_deserializer(&buff);
        let y = deserializer.load_usize().unwrap();
        assert_eq!(x, y);
        deserializer.finish().unwrap();
        true
    }
}
