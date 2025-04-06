use crate::MerkleKeyBytes;

pub trait MerkleKey {
    fn to_bytes(&self) -> MerkleKeyBytes;
}

impl MerkleKey for String {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes::from_slice(self.as_bytes())
    }
}

impl MerkleKey for str {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes::from_slice(self.as_bytes())
    }
}

impl MerkleKey for u8 {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes::from_slice(&[*self])
    }
}
impl MerkleKey for u32 {
    fn to_bytes(&self) -> MerkleKeyBytes {
        MerkleKeyBytes::from_slice(&self.to_le_bytes())
    }
}
