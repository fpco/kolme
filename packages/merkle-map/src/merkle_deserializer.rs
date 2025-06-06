use std::collections::HashSet;

use crate::*;

/// Provides context within a [MerkleDeserialize] impl for deserializing data.
pub struct MerkleDeserializer {
    hash: Sha256Hash,
    buff: Arc<[u8]>,
    pos: usize,
    manager: MerkleManager,
    children: Vec<Arc<MerkleContents>>,
}

impl MerkleDeserializer {
    pub(crate) fn new(hash: Sha256Hash, payload: Arc<[u8]>, manager: MerkleManager) -> Self {
        MerkleDeserializer {
            hash,
            buff: payload,
            pos: 0,
            manager,
            children: vec![],
        }
    }

    pub fn get_full_payload(&self) -> Arc<[u8]> {
        self.buff.clone()
    }

    pub fn get_hash(&self) -> Sha256Hash {
        self.hash
    }

    /// Look at the next byte without consuming it from the stream.
    pub fn peek_byte(&self) -> Result<u8, MerkleSerialError> {
        let byte = *self
            .buff
            .get(self.pos)
            .ok_or(MerkleSerialError::InsufficientInput)?;
        Ok(byte)
    }

    /// Get the next byte in the stream.
    pub fn pop_byte(&mut self) -> Result<u8, MerkleSerialError> {
        let byte = self.peek_byte()?;
        self.pos += 1;
        Ok(byte)
    }

    /// Load up the given number of bytes.
    pub fn load_raw_bytes(&mut self, len: usize) -> Result<&[u8], MerkleSerialError> {
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
    pub(crate) fn finish(self) -> Result<MerkleContents, MerkleSerialError> {
        if self.buff.len() == self.pos {
            Ok(MerkleContents {
                hash: self.hash,
                payload: self.buff,
                children: self.children.into(),
            })
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

    /// Load bytes and then UTF-8 decode them.
    pub fn load_str(&mut self) -> Result<&str, MerkleSerialError> {
        let bytes = self.load_bytes()?;
        std::str::from_utf8(bytes).map_err(MerkleSerialError::custom)
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

    pub(crate) fn load_by_hash_optional<T: MerkleDeserializeRaw>(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<T>, MerkleSerialError> {
        self.manager.deserialize_cached(hash)
    }

    pub fn load_by_hash<T: MerkleDeserializeRaw>(&mut self) -> Result<T, MerkleSerialError> {
        let hash = Sha256Hash::merkle_deserialize_raw(self)?;
        match self.manager.deserialize_cached(hash) {
            Err(e) => Err(e),
            Ok(Some(x)) => Ok(x),
            Ok(None) => Err(MerkleSerialError::HashesNotFound {
                hashes: HashSet::from_iter([hash]),
            }),
        }
    }

    /// Load any value that can be deserialized via [MerkleDeserialize].
    pub fn load<T: MerkleDeserializeRaw>(&mut self) -> Result<T, MerkleSerialError> {
        T::merkle_deserialize_raw(self)
    }

    pub fn load_json<T: serde::de::DeserializeOwned>(&mut self) -> Result<T, MerkleSerialError> {
        let bytes = self.load_bytes()?;
        serde_json::from_slice(bytes).map_err(MerkleSerialError::custom)
    }

    pub(crate) fn get_position(&self) -> usize {
        self.pos
    }
}
