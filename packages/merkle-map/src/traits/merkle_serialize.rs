use std::collections::{BTreeMap, BTreeSet};

use shared::{
    cryptography::{PublicKey, RecoveryId, Signature, SignatureWithRecovery},
    types::{BridgeActionId, BridgeEventId, Sha256Hash},
};

use crate::*;

impl MerkleSerialize for u8 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_byte(*self);
        Ok(())
    }
}

impl MerkleSerialize for u32 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerialize for u64 {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerialize for usize {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_usize(*self);
        Ok(())
    }
}

impl MerkleSerialize for String {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes());
        Ok(())
    }
}

impl MerkleSerialize for str {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes());
        Ok(())
    }
}

impl<T: MerkleSerialize> MerkleSerialize for Option<T> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        match self {
            Some(x) => {
                serializer.store_byte(1);
                x.merkle_serialize(serializer)
            }
            None => {
                serializer.store_byte(0);
                Ok(())
            }
        }
    }
}

impl<T: MerkleSerialize> MerkleSerialize for Vec<T> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_usize(self.len());
        for x in self {
            serializer.store(x)?;
        }
        Ok(())
    }
}

impl<T: MerkleSerialize, const N: usize> MerkleSerialize for [T; N] {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        for x in self {
            serializer.store(x)?;
        }
        Ok(())
    }
}

impl MerkleSerialize for Sha256Hash {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(self.as_array());
        Ok(())
    }
}

impl<K: MerkleSerialize, V: MerkleSerialize> MerkleSerialize for BTreeMap<K, V> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_usize(self.len());
        for (k, v) in self {
            k.merkle_serialize(serializer)?;
            v.merkle_serialize(serializer)?;
        }
        Ok(())
    }
}

impl<T: MerkleSerialize> MerkleSerialize for BTreeSet<T> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_usize(self.len());
        for x in self {
            x.merkle_serialize(serializer)?;
        }
        Ok(())
    }
}

impl MerkleSerialize for rust_decimal::Decimal {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_array(self.serialize());
        Ok(())
    }
}

impl MerkleSerialize for PublicKey {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(&self.as_bytes());
        Ok(())
    }
}

impl MerkleSerialize for BridgeEventId {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        self.0.merkle_serialize(serializer)
    }
}

impl MerkleSerialize for BridgeActionId {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        self.0.merkle_serialize(serializer)
    }
}

impl MerkleSerialize for Signature {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes().as_ref());
        Ok(())
    }
}

impl MerkleSerialize for RecoveryId {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store_byte(self.to_byte());
        Ok(())
    }
}

impl MerkleSerialize for SignatureWithRecovery {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self { recid, sig } = self;
        serializer.store(recid)?;
        serializer.store(sig)?;
        Ok(())
    }
}

impl<T1: MerkleSerialize, T2: MerkleSerialize> MerkleSerialize for (T1, T2) {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        serializer.store(&self.0)?;
        serializer.store(&self.1)?;
        Ok(())
    }
}
