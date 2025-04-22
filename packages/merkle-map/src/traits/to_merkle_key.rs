use shared::{cryptography::PublicKey, types::BridgeActionId};

use crate::*;

impl ToMerkleKey for Vec<u8> {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(self)
    }
}

impl ToMerkleKey for [u8] {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(self)
    }
}

impl ToMerkleKey for String {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(self.as_bytes())
    }
}

impl ToMerkleKey for str {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(self.as_bytes())
    }
}

impl ToMerkleKey for &str {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(self.as_bytes())
    }
}

impl ToMerkleKey for u8 {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(&[*self])
    }
}
impl ToMerkleKey for u32 {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(&self.to_le_bytes())
    }
}
impl ToMerkleKey for u64 {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(&self.to_le_bytes())
    }
}

impl ToMerkleKey for PublicKey {
    fn to_merkle_key(&self) -> MerkleKey {
        MerkleKey::from_slice(&self.as_bytes())
    }
}

impl ToMerkleKey for BridgeActionId {
    fn to_merkle_key(&self) -> MerkleKey {
        self.0.to_merkle_key()
    }
}
