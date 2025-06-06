use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use shared::{
    cryptography::{PublicKey, RecoveryId, Signature, SignatureWithRecovery},
    types::{BridgeActionId, BridgeEventId, Sha256Hash, ValidatorSet},
};

use jiff::Timestamp;

use smallvec::{Array, SmallVec};

use crate::*;

impl MerkleSerializeRaw for u8 {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_byte(*self);
        Ok(())
    }
}

impl MerkleSerializeRaw for u16 {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerializeRaw for u32 {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerializeRaw for u64 {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerializeRaw for u128 {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.to_le_bytes());
        Ok(())
    }
}

impl MerkleSerializeRaw for usize {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_usize(*self);
        Ok(())
    }
}

impl MerkleSerializeRaw for String {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes());
        Ok(())
    }
}

impl MerkleSerializeRaw for str {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes());
        Ok(())
    }
}

impl<T: MerkleSerializeRaw> MerkleSerializeRaw for Option<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        match self {
            Some(x) => {
                serializer.store_byte(1);
                x.merkle_serialize_raw(serializer)
            }
            None => {
                serializer.store_byte(0);
                Ok(())
            }
        }
    }
}

// TODO any way we can get this impl back?
// impl<T: MerkleSerializeRaw + ?Sized> MerkleSerializeRaw for Box<T> {
//     fn merkle_serialize_raw(
//         &self,
//         serializer: &mut MerkleSerializer,
//     ) -> Result<(), MerkleSerialError> {
//         self.as_ref().merkle_serialize_raw(serializer)
//     }
// }

impl<T: MerkleSerializeRaw + ?Sized> MerkleSerializeRaw for Rc<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        self.as_ref().merkle_serialize_raw(serializer)
    }
}

impl<T: MerkleSerializeRaw + ?Sized> MerkleSerializeRaw for Arc<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        self.as_ref().merkle_serialize_raw(serializer)
    }
}

impl<T: MerkleSerializeRaw> MerkleSerializeRaw for Vec<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        self.as_slice().merkle_serialize_raw(serializer)
    }
}

impl<T: MerkleSerializeRaw> MerkleSerializeRaw for [T] {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_usize(self.len());
        for x in self {
            serializer.store(x)?;
        }
        Ok(())
    }
}

impl<T: MerkleSerializeRaw, const N: usize> MerkleSerializeRaw for [T; N] {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        for x in self {
            serializer.store(x)?;
        }
        Ok(())
    }
}

impl MerkleSerializeRaw for Sha256Hash {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(self.as_array());
        Ok(())
    }
}

impl<K: MerkleSerializeRaw, V: MerkleSerializeRaw> MerkleSerializeRaw for BTreeMap<K, V> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_usize(self.len());
        for (k, v) in self {
            k.merkle_serialize_raw(serializer)?;
            v.merkle_serialize_raw(serializer)?;
        }
        Ok(())
    }
}

impl<T: MerkleSerializeRaw> MerkleSerializeRaw for BTreeSet<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_usize(self.len());
        for x in self {
            x.merkle_serialize_raw(serializer)?;
        }
        Ok(())
    }
}

impl MerkleSerializeRaw for rust_decimal::Decimal {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_array(self.serialize());
        Ok(())
    }
}

impl MerkleSerializeRaw for PublicKey {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_slice(&self.as_bytes());
        Ok(())
    }
}

impl MerkleSerializeRaw for BridgeEventId {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        self.0.merkle_serialize_raw(serializer)
    }
}

impl MerkleSerializeRaw for BridgeActionId {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        self.0.merkle_serialize_raw(serializer)
    }
}

impl MerkleSerializeRaw for Signature {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_slice(self.as_bytes().as_ref());
        Ok(())
    }
}

impl MerkleSerializeRaw for RecoveryId {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_byte(self.to_byte());
        Ok(())
    }
}

impl MerkleSerializeRaw for SignatureWithRecovery {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
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

impl MerkleSerializeRaw for ValidatorSet {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        let Self {
            processor,
            listeners,
            needed_listeners,
            approvers,
            needed_approvers,
        } = self;
        serializer.store(processor)?;
        serializer.store(listeners)?;
        serializer.store(needed_listeners)?;
        serializer.store(approvers)?;
        serializer.store(needed_approvers)?;
        Ok(())
    }
}

impl MerkleSerializeRaw for bool {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_byte(if *self { 1 } else { 0 });
        Ok(())
    }
}

impl MerkleSerializeRaw for Timestamp {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store_raw_bytes(&self.as_nanosecond().to_le_bytes());
        Ok(())
    }
}

impl<A: Array<Item: MerkleSerializeRaw>> MerkleSerializeRaw for SmallVec<A> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(self.as_slice())?;
        Ok(())
    }
}
