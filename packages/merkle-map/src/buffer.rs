//! Helper types and functions for buffer reading and writing.

use shared::types::Sha256Hash;

use crate::LoadMerkleMapError;

#[derive(Default)]
pub(crate) struct WriteBuffer {
    buff: Vec<u8>,
}

impl From<Vec<u8>> for WriteBuffer {
    fn from(buff: Vec<u8>) -> Self {
        Self { buff }
    }
}

impl WriteBuffer {
    pub(crate) fn finish(self) -> Vec<u8> {
        self.buff
    }

    pub(crate) fn store_usize(&mut self, mut value: usize) {
        if value == 0 {
            self.buff.push(0);
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
                self.buff.push(bytes[i]);
            } else {
                self.buff.push(bytes[i] | 0x80);
            }
        }
    }

    pub(crate) fn store_slice(&mut self, bytes: &[u8]) {
        self.store_usize(bytes.len());
        self.buff.extend_from_slice(bytes);
    }

    pub(crate) fn push(&mut self, byte: u8) {
        self.buff.push(byte);
    }

    pub(crate) fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.buff.extend_from_slice(bytes)
    }
}

pub(crate) struct ReadBuffer<'a> {
    buff: &'a [u8],
    pos: usize,
}

impl<'a> ReadBuffer<'a> {
    pub(crate) fn new(buff: &'a [u8]) -> Self {
        ReadBuffer { buff, pos: 0 }
    }

    pub(crate) fn pop_byte<StoreError>(&mut self) -> Result<u8, LoadMerkleMapError<StoreError>> {
        let byte = *self
            .buff
            .get(self.pos)
            .ok_or(LoadMerkleMapError::InsufficientInput)?;
        self.pos += 1;
        Ok(byte)
    }

    pub(crate) fn load_usize<StoreError>(
        &mut self,
    ) -> Result<usize, LoadMerkleMapError<StoreError>> {
        let mut value = 0usize;

        loop {
            let byte = self.pop_byte()?;

            if value > (usize::MAX >> 7) {
                // Overflow, do something better?
                return Err(LoadMerkleMapError::UsizeOverflow);
            }

            value = (value << 7) | (byte & 0x7F) as usize;
            if byte & 0x80 == 0 {
                // If no continuation bit, this was the last byte
                return Ok(value);
            }
        }
    }

    pub(crate) fn load_hash<StoreError>(
        &mut self,
    ) -> Result<Sha256Hash, LoadMerkleMapError<StoreError>> {
        if self.pos + 32 <= self.buff.len() {
            let hash =
                Sha256Hash::from_array(self.buff[self.pos..self.pos + 32].try_into().unwrap());
            self.pos += 32;
            Ok(hash)
        } else {
            Err(LoadMerkleMapError::InsufficientInput)
        }
    }

    pub(crate) fn load_bytes<StoreError>(
        &mut self,
    ) -> Result<&[u8], LoadMerkleMapError<StoreError>> {
        let len = self.load_usize()?;
        let end = self.pos + len;
        if end > self.buff.len() {
            Err(LoadMerkleMapError::InsufficientInput)
        } else {
            let slice = &self.buff[self.pos..end];
            self.pos = end;
            Ok(slice)
        }
    }

    pub(crate) fn finish<StoreError>(self) -> Result<(), LoadMerkleMapError<StoreError>> {
        if self.buff.len() == self.pos {
            Ok(())
        } else {
            Err(LoadMerkleMapError::TooMuchInput)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    quickcheck::quickcheck! {
        fn test_store_usize(x: usize) -> bool {
            test_store_usize_inner(x)
        }
    }

    fn test_store_usize_inner(x: usize) -> bool {
        let mut buff = WriteBuffer::default();
        buff.store_usize(x);
        let buff = buff.finish();
        let mut buff = ReadBuffer {
            buff: &buff,
            pos: 0,
        };
        let y = buff.load_usize::<()>().unwrap();
        assert_eq!(x, y);
        assert_eq!(buff.pos, buff.buff.len());
        true
    }
}
