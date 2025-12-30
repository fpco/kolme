use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use jiff::Timestamp;
use shared::{
    cryptography::{PublicKey, RecoveryId, Signature, SignatureWithRecovery},
    types::{BridgeActionId, BridgeEventId, Sha256Hash, ValidatorSet},
};
use smallvec::{Array, SmallVec};

use crate::*;

impl MerkleDeserializeRaw for u8 {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.pop_byte()
    }
}

impl MerkleDeserializeRaw for u16 {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u16::from_le_bytes)
    }
}

impl MerkleDeserializeRaw for u32 {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u32::from_le_bytes)
    }
}

impl MerkleDeserializeRaw for u64 {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u64::from_le_bytes)
    }
}

impl MerkleDeserializeRaw for u128 {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(u128::from_le_bytes)
    }
}

impl MerkleDeserializeRaw for usize {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_usize()
    }
}

impl MerkleDeserializeRaw for String {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let bytes = deserializer.load_bytes()?;
        Ok(std::str::from_utf8(bytes)?.to_owned())
    }
}

impl MerkleDeserializeRaw for Arc<str> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        String::merkle_deserialize_raw(deserializer).map(Into::into)
    }
}

impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for Option<T> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        match deserializer.pop_byte()? {
            0 => Ok(None),
            1 => T::merkle_deserialize_raw(deserializer).map(Some),
            x => Err(MerkleSerialError::InvalidOptionByte { byte: x }),
        }
    }
}

// TODO any way to get this impl back?
// impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for Box<T> {
//     fn merkle_deserialize_raw(
//         deserializer: &mut MerkleDeserializer,
//     ) -> Result<Self, MerkleSerialError> {
//         T::merkle_deserialize_raw(deserializer).map(Into::into)
//     }
// }

impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for Rc<T> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        T::merkle_deserialize_raw(deserializer).map(Into::into)
    }
}

impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for Arc<T> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        T::merkle_deserialize_raw(deserializer).map(Into::into)
    }
}

impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for Vec<T> {
    fn merkle_deserialize_raw(
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

impl<T: MerkleDeserializeRaw> MerkleDeserializeRaw for Arc<[T]> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Vec::<T>::merkle_deserialize_raw(deserializer).map(Into::into)
    }
}

impl<T: MerkleDeserializeRaw, const N: usize> MerkleDeserializeRaw for [T; N] {
    fn merkle_deserialize_raw(
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

impl MerkleDeserializeRaw for Sha256Hash {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer.load_array().map(Sha256Hash::from_array)
    }
}

impl<K: MerkleDeserializeRaw + Ord, V: MerkleDeserializeRaw> MerkleDeserializeRaw
    for BTreeMap<K, V>
{
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let len = deserializer.load_usize()?;
        let mut x = BTreeMap::new();
        for _ in 0..len {
            let k = K::merkle_deserialize_raw(deserializer)?;
            let v = V::merkle_deserialize_raw(deserializer)?;
            x.insert(k, v);
        }
        Ok(x)
    }
}

impl<T: MerkleDeserializeRaw + Ord> MerkleDeserializeRaw for BTreeSet<T> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let len = deserializer.load_usize()?;
        let mut x = BTreeSet::new();
        for _ in 0..len {
            let t = T::merkle_deserialize_raw(deserializer)?;
            x.insert(t);
        }
        Ok(x)
    }
}

impl MerkleDeserializeRaw for rust_decimal::Decimal {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        deserializer
            .load_array()
            .map(rust_decimal::Decimal::deserialize)
    }
}

impl MerkleDeserializeRaw for PublicKey {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let bytes = deserializer.load_bytes()?;
        PublicKey::from_bytes(bytes).map_err(MerkleSerialError::custom)
    }
}

impl MerkleDeserializeRaw for BridgeEventId {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        u64::merkle_deserialize_raw(deserializer).map(Self)
    }
}

impl MerkleDeserializeRaw for BridgeActionId {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        u64::merkle_deserialize_raw(deserializer).map(Self)
    }
}

impl MerkleDeserializeRaw for Signature {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Signature::from_slice(deserializer.load_bytes()?).map_err(MerkleSerialError::custom)
    }
}

impl MerkleDeserializeRaw for RecoveryId {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let byte = deserializer.pop_byte()?;
        RecoveryId::from_byte(byte).map_err(|_| MerkleSerialError::InvalidRecoveryId { byte })
    }
}

impl MerkleDeserializeRaw for SignatureWithRecovery {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            recid: deserializer.load()?,
            sig: deserializer.load()?,
        })
    }
}

impl<T1: MerkleDeserializeRaw, T2: MerkleDeserializeRaw> MerkleDeserializeRaw for (T1, T2) {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok((deserializer.load()?, deserializer.load()?))
    }
}

impl MerkleDeserializeRaw for ValidatorSet {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            processor: deserializer.load()?,
            listeners: deserializer.load()?,
            needed_listeners: deserializer.load()?,
            approvers: deserializer.load()?,
            needed_approvers: deserializer.load()?,
        })
    }
}

impl MerkleDeserializeRaw for bool {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(deserializer.pop_byte()? == 1)
    }
}

impl MerkleDeserializeRaw for Timestamp {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let as_bytes: [u8; 128 / 8] = deserializer.load_array()?;
        Timestamp::from_nanosecond(i128::from_le_bytes(as_bytes))
            .map_err(MerkleSerialError::InvalidTimestamp)
    }
}

impl<A: Array<Item: MerkleDeserializeRaw>> MerkleDeserializeRaw for SmallVec<A> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        // SmallVec is serialized as slice, so it is preceded by length
        let size: usize = deserializer.load()?;
        let mut result = SmallVec::with_capacity(A::size());
        for _ in 0..size {
            result.push(deserializer.load()?)
        }
        Ok(result)
    }
}

impl MerkleDeserializeRaw for () {
    fn merkle_deserialize_raw(
        _deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        Ok(())
    }
}
