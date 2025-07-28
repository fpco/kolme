use std::collections::HashSet;

use crate::*;

/// Provides context within a [MerkleDeserialize] impl for deserializing data.
pub struct MerkleDeserializer {
    contents: Arc<MerkleContents>,
    pos: usize,
}

impl MerkleDeserializer {
    pub(crate) fn new(contents: Arc<MerkleContents>) -> Self {
        MerkleDeserializer { contents, pos: 0 }
    }

    pub fn get_full_payload(&self) -> Arc<[u8]> {
        self.contents.payload.clone()
    }

    pub fn get_hash(&self) -> Sha256Hash {
        self.contents.hash
    }

    /// Look at the next byte without consuming it from the stream.
    pub fn peek_byte(&self) -> Result<u8, MerkleSerialError> {
        let byte = *self
            .contents
            .payload
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
        if end > self.contents.payload.len() {
            Err(MerkleSerialError::InsufficientInput)
        } else {
            let slice = &self.contents.payload[self.pos..end];
            self.pos = end;
            Ok(slice)
        }
    }

    /// Finish processing, ensuring that all input was consumed.
    pub(crate) fn finish(self) -> Result<Arc<MerkleContents>, MerkleSerialError> {
        if self.contents.payload.len() == self.pos {
            Ok(self.contents)
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

    pub fn load_by_hash<T: MerkleDeserializeRaw>(&mut self) -> Result<T, MerkleSerialError> {
        let hash = Sha256Hash::merkle_deserialize_raw(self)?;
        self.load_by_given_hash(hash)
    }

    pub(crate) fn load_by_given_hash<T: MerkleDeserializeRaw>(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<T, MerkleSerialError> {
        // NOTE: using a find here instead of reconstituting the children into a HashMap.
        // The assumption is that the number of children will generally be small enough
        // that a scan of the slice is faster than a HashMap lookup, and will involve
        // less allocation. If this assumption turns out to be wrong in the future,
        // we can create a HashMap of all the children when creating the MerkleDeserializer.
        match self.contents.children.iter().find(|c| c.hash == hash) {
            None => Err(MerkleSerialError::HashesNotFound {
                hashes: HashSet::from_iter([hash]),
            }),
            Some(child) => crate::api::deserialize(child.clone()),
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

    /// Ignore the remaining data, not generating an error on unconsumed input.
    ///
    /// Useful for migrating to a new data format that drops old fields.
    pub fn ignore_remaining(&mut self) {
        self.pos = self.contents.payload.len()
    }
}
