mod from_merkle_key;
mod merkle_deserialize;
mod merkle_serialize;
mod to_merkle_key;

use std::sync::Arc;

use shared::types::Sha256Hash;

use crate::*;

/// Values which can be used as keys in a [crate::MerkleMap].
pub trait ToMerkleKey {
    fn to_merkle_key(&self) -> MerkleKey;
}

/// Values which can be parsed back from a rendered merkle key.
///
/// This trait is kept separate from [ToMerkleKey] to allow
/// rendering keys from non-[Sized] types.
pub trait FromMerkleKey: Sized {
    fn from_merkle_key(bytes: &[u8]) -> Result<Self, MerkleSerialError>;
}

/// A value that can be serialized within a [MerkleMap].
pub trait MerkleSerialize {
    /// Serialize this data for storage.
    fn serialize<S: MerkleSerializer>(
        &mut self,
        serializer: &mut S,
    ) -> Result<(), MerkleSerialError>;
}

pub trait MerkleSerializeComplete {
    fn serialize_complete<Store: MerkleStore>(
        &mut self,
        manager: &MerkleManager<Store>,
    ) -> Result<Sha256Hash, MerkleSerialError>;
}

impl<T: MerkleSerialize> MerkleSerializeComplete for T {
    fn serialize_complete<Store: MerkleStore>(
        &mut self,
        manager: &MerkleManager<Store>,
    ) -> Result<Sha256Hash, MerkleSerialError> {
        let mut serializer = manager.new_serializer();
        self.serialize(&mut serializer)?;
        let (hash, bytes) = serializer.finish()?;
        manager.save_merkle_by_hash(hash, bytes)?;
        Ok(hash)
    }
}

/// Provides context within a [MerkleSerialize] impl for serializing data.
pub trait MerkleSerializer {
    /// Store a single byte.
    fn store_byte(&mut self, byte: u8);

    /// Store raw bytes without any length encoding.
    ///
    /// This can be used as an optimization for calling [MerkleSerializer::store_byte] repeatedly.
    fn store_raw_bytes(&mut self, bytes: &[u8]) {
        bytes.iter().copied().for_each(|b| self.store_byte(b))
    }

    /// Store the size of the buffer followed by the bytes.
    fn store_slice(&mut self, bytes: &[u8]) {
        self.store_usize(bytes.len());
        self.store_raw_bytes(bytes);
    }

    /// Variable-length encoding of a usize.
    fn store_usize(&mut self, mut value: usize) {
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

    /// Create a fresh serializer for serializing a subcomponent.
    fn new_serializer(&self) -> Self;

    /// Finish generating the output and return the completed buffer.
    fn finish(self) -> Result<(Sha256Hash, Arc<[u8]>), MerkleSerialError>;
}

/// A value that can be deserialized back into a [MerkleMap] value.
pub trait MerkleDeserialize: Sized {
    fn deserialize<D: MerkleDeserializer>(deserializer: &mut D) -> Result<Self, MerkleSerialError>;
}

/// Provides context within a [MerkleDeserialize] impl for deserializing data.
pub trait MerkleDeserializer {
    /// Get the next byte in the stream.
    fn pop_byte(&mut self) -> Result<u8, MerkleSerialError>;

    /// Load up the given number of bytes.
    fn load_raw_bytes(&mut self, len: usize) -> Result<&[u8], MerkleSerialError>;

    /// Load an array with a fixed number of bytes
    fn load_array<const N: usize>(&mut self) -> Result<[u8; N], MerkleSerialError> {
        self.load_raw_bytes(N).map(|x| x.try_into().unwrap())
    }

    /// Get the bytes stored by a [MerkleSerializer::store_slioce]
    fn load_bytes(&mut self) -> Result<&[u8], MerkleSerialError> {
        let len = self.load_usize()?;
        self.load_raw_bytes(len)
    }

    fn load_usize(&mut self) -> Result<usize, MerkleSerialError> {
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

    /// Finish processing, ensuring that all input was consumed.
    fn finish(self) -> Result<(), MerkleSerialError>;
}

/// A backing store for raw blobs used by a [MerkleMap].
pub trait MerkleStore {
    /// Load up the blob by hash, if available.
    fn load_merkle_by_hash(&self, hash: Sha256Hash)
        -> Result<Option<Arc<[u8]>>, MerkleSerialError>;

    /// Save the payload within the Merkle store.
    ///
    /// Invariant: the hash must be the correct hash of the given payload.
    fn save_merkle_by_hash(
        &self,
        hash: Sha256Hash,
        payload: &[u8],
    ) -> Result<(), MerkleSerialError>;

    /// Checks if the store already has a blob matching the given hash.
    fn contains_hash(&self, hash: Sha256Hash) -> Result<bool, MerkleSerialError>;
}
