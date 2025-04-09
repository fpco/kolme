//! Helper types and functions for buffer reading and writing.

use std::{collections::VecDeque, sync::Arc};

use shared::types::Sha256Hash;

use crate::*;

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

/// Manages overall serialization of potential multiple subtrees.
pub struct MerkleSerializeManager {
    contents: VecDeque<SingleMerkleContents>,
}

struct SingleMerkleContents {
    hash: Sha256Hash,
    payload: Arc<[u8]>,
    children: Vec<Box<dyn MerkleSerializeComplete>>,
}

impl MerkleSerializeManager {
    pub fn add_contents(
        &mut self,
        hash: Sha256Hash,
        payload: Arc<[u8]>,
        children: Vec<Box<dyn MerkleSerializeComplete>>,
    ) {
        self.contents.push_back(SingleMerkleContents {
            hash,
            payload,
            children,
        });
    }
}

/// Provides context within a [MerkleSerialize] impl for serializing data.
pub struct MerkleSerializer {
    buff: Vec<u8>,
}

impl MerkleSerializeManager {
    pub async fn save<Store: MerkleStore>(
        mut self,
        store: &mut Store,
    ) -> Result<(), MerkleSerialError> {
        loop {
            let Some(SingleMerkleContents {
                hash,
                payload,
                children,
            }) = self.contents.pop_front()
            else {
                break Ok(());
            };
            if !store.contains_hash(hash).await? {
                // Always save the children first to ensure our data store
                // doesn't end up with dangling references
                if children.is_empty() {
                    store.save_merkle_by_hash(hash, &payload).await?;
                } else {
                    for child in children {
                        child.serialize_complete(&mut self)?;
                    }
                    self.contents.push_back(SingleMerkleContents {
                        hash,
                        payload,
                        children: vec![],
                    });
                }
            }
        }
    }
}

/// Serialize a value to a set of hashes and payloads to include in storage.
pub fn merkle_serialize<T: MerkleSerializeComplete>(
    value: &T,
) -> Result<MerkleSerializeManager, MerkleSerialError> {
    let mut manager = MerkleSerializeManager {
        contents: VecDeque::new(),
    };
    value.serialize_complete(&mut manager)?;
    Ok(manager)
}

impl MerkleSerializer {
    pub(crate) fn new() -> MerkleSerializer {
        MerkleSerializer { buff: vec![] }
    }

    /// Store a single byte.
    pub fn store_byte(&mut self, byte: u8) {
        self.buff.push(byte);
    }

    /// Store raw bytes without any length encoding.
    ///
    /// This can be used as an optimization for calling [MerkleSerializer::store_byte] repeatedly.
    pub fn store_raw_bytes(&mut self, bytes: &[u8]) {
        self.buff.extend_from_slice(bytes);
    }

    /// Finish generating the output and return the completed buffer.
    pub fn finish(self) -> (Sha256Hash, Arc<[u8]>) {
        let buff = Arc::<[u8]>::from(self.buff);
        let hash = Sha256Hash::hash(&buff);
        (hash, buff)
    }

    // FIXME
    // fn new_serializer(&self) -> Self {
    //     self.manager.new_serializer()
    // }

    /// Store the size of the buffer followed by the bytes.
    pub fn store_slice(&mut self, bytes: &[u8]) {
        self.store_usize(bytes.len());
        self.store_raw_bytes(bytes);
    }

    /// Variable-length encoding of a usize.
    pub fn store_usize(&mut self, mut value: usize) {
        if value == 0 {
            self.store_byte(0);
            return;
        }

        let mut bytes = [0u8; 10];
        let mut next = 0;

        // First pass: collect 7-bit chunks
        while value > 0 {
            let chunk = (value & 0x7F) as u8; // Take lowest 7 bits
            bytes[next] = chunk;
            next += 1;
            value >>= 7; // Shift right by 7 bits
        }

        for i in (0..next).rev() {
            if i == 0 {
                self.store_byte(bytes[i]);
            } else {
                self.store_byte(bytes[i] | 0x80);
            }
        }
    }

    /// Store a JSON-encoded version of this content.
    pub fn store_json<T: serde::Serialize>(&mut self, t: &T) -> Result<(), MerkleSerialError> {
        let bytes = serde_json::to_vec(t).map_err(MerkleSerialError::custom)?;
        self.store_slice(&bytes);
        Ok(())
    }

    // Serialize the given value, store it in the Merkle store, and write the hash to the current serialization.
    // #[allow(async_fn_in_trait)]
    // async fn store_by_merkle_hash<T: MerkleSerializeComplete + ?Sized>(
    //     &mut self,
    //     t: &mut T,
    // ) -> Result<(), MerkleSerialError> {
    //     let mut hash = t.serialize_complete(self.get_manager()).await?;
    //     hash.serialize(self).await?;
    //     Ok(())
    // }
    // FIXME

    // Create a fresh serializer for serializing a subcomponent.
    // fn new_serializer(&self) -> Self
}

/// Provides context within a [MerkleDeserialize] impl for deserializing data.
pub struct MerkleDeserializer<'a> {
    pub(crate) buff: &'a [u8],
    pub(crate) pos: usize,
    // // TODO we'll probably need this so we can add a helper method to MerkleDeserializer
    // #[allow(dead_code)]
    // pub(crate) manager: MerkleManager<Store>,
}

impl<'a> MerkleDeserializer<'a> {
    /// Get the next byte in the stream.
    pub fn pop_byte(&mut self) -> Result<u8, MerkleSerialError> {
        let byte = *self
            .buff
            .get(self.pos)
            .ok_or(MerkleSerialError::InsufficientInput)?;
        self.pos += 1;
        Ok(byte)
    }

    /// Load up the given number of bytes.
    pub fn load_raw_bytes(&mut self, len: usize) -> Result<&'a [u8], MerkleSerialError> {
        let end = self.pos + len;
        if end > self.buff.len() {
            Err(MerkleSerialError::InsufficientInput)
        } else {
            let slice = &self.buff[self.pos..end];
            self.pos = end;
            Ok(slice)
        }
    }

    /// Finish processing, ensuring that all input was consumed.
    pub fn finish(self) -> Result<(), MerkleSerialError> {
        if self.buff.len() == self.pos {
            Ok(())
        } else {
            Err(MerkleSerialError::TooMuchInput)
        }
    }

    /// Load an array with a fixed number of bytes
    pub fn load_array<const N: usize>(&mut self) -> Result<[u8; N], MerkleSerialError> {
        self.load_raw_bytes(N).map(|x| x.try_into().unwrap())
    }

    /// Get the bytes stored by a [MerkleSerializer::store_slioce]
    pub fn load_bytes(&mut self) -> Result<&[u8], MerkleSerialError> {
        let len = self.load_usize()?;
        self.load_raw_bytes(len)
    }

    pub fn load_usize(&mut self) -> Result<usize, MerkleSerialError> {
        let mut value = 0usize;

        loop {
            let byte = self.pop_byte()?;

            if value > (usize::MAX >> 7) {
                // Overflow, do something better?
                return Err(MerkleSerialError::UsizeOverflow);
            }

            value = (value << 7) | (byte & 0x7F) as usize;
            if byte & 0x80 == 0 {
                // If no continuation bit, this was the last byte
                return Ok(value);
            }
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

    use super::*;

    quickcheck::quickcheck! {
        fn test_store_usize(x: usize) -> bool {
            test_store_usize_inner(x)
        }
    }

    #[tokio::main]
    async fn test_store_usize_inner(x: usize) -> bool {
        let mut serializer = MerkleSerializer { buff: vec![] };
        serializer.store_usize(x);
        let (_hash, buff) = serializer.finish();
        let mut deserializer = MerkleDeserializer {
            buff: &buff,
            pos: 0,
        };
        let y = deserializer.load_usize().unwrap();
        assert_eq!(x, y);
        deserializer.finish().unwrap();
        true
    }
}
