use std::collections::{BTreeMap, BTreeSet};

use shared::{
    cryptography::{PublicKey, RecoveryId, Signature, SignatureWithRecovery},
    types::{BridgeActionId, BridgeEventId, Sha256Hash},
};

use crate::*;

impl MerkleDeserialize for u8 {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.pop_byte()
    }
}

impl MerkleDeserialize for u32 {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u32::from_le_bytes)
    }
}

impl MerkleDeserialize for u64 {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u64::from_le_bytes)
    }
}

impl MerkleDeserialize for usize {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_usize()
    }
}

impl MerkleDeserialize for String {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let bytes = deserializer.load_bytes()?;
        std::str::from_utf8(bytes)
            .map(ToOwned::to_owned)
            .map_err(MerkleSerialError::custom)
    }
}

impl<T: MerkleDeserialize> MerkleDeserialize for Option<T> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        match deserializer.pop_byte()? {
            0 => Ok(None),
            1 => T::merkle_deserialize(deserializer).map(Some),
            x => Err(MerkleSerialError::Other(format!(
                "When deserializing an Option, invalid byte {x}"
            ))),
        }
    }
}

impl<T: MerkleDeserialize> MerkleDeserialize for Vec<T> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let len = deserializer.load_usize()?;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(deserializer.load()?);
        }
        Ok(v)
    }
}

impl<T: MerkleDeserialize, const N: usize> MerkleDeserialize for [T; N] {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let mut v = Vec::with_capacity(N);
        for _ in 0..N {
            v.push(deserializer.load()?);
        }
        Ok(match v.try_into() {
            Ok(x) => x,
            Err(_) => unreachable!(),
        })
    }
}

impl MerkleDeserialize for Sha256Hash {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(Sha256Hash::from_array)
    }
}

impl<K: MerkleDeserialize + Ord, V: MerkleDeserialize> MerkleDeserialize for BTreeMap<K, V> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let len = deserializer.load_usize()?;
        let mut x = BTreeMap::new();
        for _ in 0..len {
            let k = K::merkle_deserialize(deserializer)?;
            let v = V::merkle_deserialize(deserializer)?;
            x.insert(k, v);
        }
        Ok(x)
    }
}

impl<T: MerkleDeserialize + Ord> MerkleDeserialize for BTreeSet<T> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let len = deserializer.load_usize()?;
        let mut x = BTreeSet::new();
        for _ in 0..len {
            let t = T::merkle_deserialize(deserializer)?;
            x.insert(t);
        }
        Ok(x)
    }
}

impl MerkleDeserialize for rust_decimal::Decimal {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer
            .load_array()
            .map(rust_decimal::Decimal::deserialize)
    }
}

impl MerkleDeserialize for PublicKey {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let bytes = deserializer.load_bytes()?;
        PublicKey::from_bytes(bytes).map_err(MerkleSerialError::custom)
    }
}

impl MerkleDeserialize for BridgeEventId {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        u64::merkle_deserialize(deserializer).map(Self)
    }
}

impl MerkleDeserialize for BridgeActionId {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        u64::merkle_deserialize(deserializer).map(Self)
    }
}

impl MerkleDeserialize for Signature {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Signature::from_slice(deserializer.load_bytes()?).map_err(MerkleSerialError::custom)
    }
}

impl MerkleDeserialize for RecoveryId {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let byte = deserializer.pop_byte()?;
        RecoveryId::from_byte(byte).map_err(MerkleSerialError::custom)
    }
}

impl MerkleDeserialize for SignatureWithRecovery {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            recid: deserializer.load()?,
            sig: deserializer.load()?,
        })
    }
}

impl<T1: MerkleDeserialize, T2: MerkleDeserialize> MerkleDeserialize for (T1, T2) {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok((deserializer.load()?, deserializer.load()?))
    }
}
