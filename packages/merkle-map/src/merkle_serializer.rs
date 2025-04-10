use crate::*;

/// Provides context within a [MerkleSerialize] impl for serializing data.
pub struct MerkleSerializer {
    buff: Vec<u8>,
    children: Vec<Arc<MerkleContents>>,
    manager: MerkleManager,
}

impl MerkleSerializer {
    pub(crate) fn new(manager: MerkleManager) -> Self {
        MerkleSerializer {
            buff: vec![],
            children: vec![],
            manager,
        }
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

    /// Store an array as raw bytes.
    pub fn store_array<const N: usize>(&mut self, array: [u8; N]) {
        self.store_raw_bytes(&array);
    }

    /// Finish generating the output and return the completed buffer.
    pub(crate) fn finish(self) -> MerkleContents {
        let MerkleSerializer {
            buff,
            children,
            manager: _,
        } = self;
        let buff = Arc::<[u8]>::from(buff);
        let hash = Sha256Hash::hash(&buff);
        MerkleContents {
            hash,
            payload: buff,
            children: children.into(),
        }
    }

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

    /// Store any value that can be serialized via [MerkleSerialize].
    pub fn store<T: MerkleSerialize>(&mut self, value: &T) -> Result<(), MerkleSerialError> {
        value.serialize(self)
    }

    /// Store a JSON-encoded version of this content.
    pub fn store_json<T: serde::Serialize>(&mut self, t: &T) -> Result<(), MerkleSerialError> {
        let bytes = serde_json::to_vec(t).map_err(MerkleSerialError::custom)?;
        self.store_slice(&bytes);
        Ok(())
    }

    /// Serialize as a new top level stored value and then store the hash in the current serialization.
    pub fn store_by_hash<T: MerkleSerialize>(
        &mut self,
        child: &T,
    ) -> Result<(), MerkleSerialError> {
        let contents = self.manager.serialize(child)?;
        let hash = contents.hash;
        self.children.push(contents);
        hash.serialize(self)
    }
}
