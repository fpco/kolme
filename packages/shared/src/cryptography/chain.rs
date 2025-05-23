use std::fmt::Display;

#[cfg(feature = "cosmwasm")]
use cosmwasm_std::HexBinary;

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
use hex;

#[derive(thiserror::Error, Debug)]
pub enum PublicKeyError {}

#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize),
)]
#[derive(Debug, PartialEq, Eq, Hash, Clone, PartialOrd, Ord, Copy)]
pub struct PublicKey(pub [u8; 33]);

impl PublicKey {
    pub fn from_bytes(bytes: [u8; 33]) -> Result<Self, PublicKeyError> {
        Ok(PublicKey(bytes))
    }
}

#[cfg(feature = "cosmwasm")]
impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        HexBinary::from(&self.0).fmt(f)
    }
}


#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&hex::encode(&self.0))
    }
}

#[cfg(feature = "cosmwasm")]
impl serde::Serialize for PublicKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        HexBinary::from(&self.0).serialize(serializer)
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl serde::Serialize for PublicKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "cosmwasm")]
impl<'de> serde::Deserialize<'de> for PublicKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let binary = HexBinary::deserialize(deserializer)?;
        let array = binary
            .to_array()
            .map_err(|_| serde::de::Error::custom("Incorrect size of public key data"))?;

        Ok(Self(array))
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl<'de> serde::Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        let mut bytes = [0u8; 33];
        hex::decode_to_slice(s, &mut bytes as &mut [u8]).map_err(serde::de::Error::custom)?;

        Ok(Self(bytes))
    }
}

#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize),
)]
#[derive(Debug, PartialEq, Eq, Hash, Clone, PartialOrd, Ord, Copy)]
pub struct PublicKeyUncompressed(pub [u8; 64]);

impl PublicKeyUncompressed {
    /// Returns `0x02` if y-coordinate is even and `0x03` if odd.
    pub fn y_parity(&self) -> u8 {
        if self.0[64 - 1] & 1 == 1 {
            0x03
        } else {
            0x02
        }
    }

    pub fn to_sec1_bytes(&self) -> PublicKey {
        let mut res = [0u8; 33];
        res[0] = self.y_parity();
        res[1..].copy_from_slice(&self.0.as_slice()[..32]);

        PublicKey(res)
    }
}

#[cfg(feature = "cosmwasm")]
impl Display for PublicKeyUncompressed {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        HexBinary::from(&self.0).fmt(f)
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl Display for PublicKeyUncompressed {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&hex::encode(&self.0))
    }
}

#[cfg(feature = "cosmwasm")]
impl serde::Serialize for PublicKeyUncompressed {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        HexBinary::from(&self.0).serialize(serializer)
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl serde::Serialize for PublicKeyUncompressed {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}


#[cfg(feature = "cosmwasm")]
impl<'de> serde::Deserialize<'de> for PublicKeyUncompressed {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let binary = HexBinary::deserialize(deserializer)?;
        let array = binary
            .to_array()
            .map_err(|_| serde::de::Error::custom("Incorrect size of public key data"))?;

        Ok(Self(array))
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl<'de> serde::Deserialize<'de> for PublicKeyUncompressed {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        let mut bytes = [0u8; 64];
        hex::decode_to_slice(s, &mut bytes as &mut [u8]).map_err(serde::de::Error::custom)?;

        Ok(Self(bytes))
    }
}

#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize),
)]
#[derive(
    serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy, PartialOrd, Ord,
)]
pub struct RecoveryId(u8);

impl RecoveryId {
    pub fn to_byte(&self) -> u8 {
        self.0
    }
}

impl Display for RecoveryId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize),
)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct Signature([u8; 64]);

impl Signature {
    pub fn to_bytes(&self) -> &[u8] {
        &self.0
    }

    // Ported from: https://github.com/cryspen/hacl-packages/blob/05c3d8fb321ed65e3db3a6a8b853019e86fb40a2/src/msvc/Hacl_K256_ECDSA.c#L1772
    /// Returns `true` if S is low-S normalized and `false` otherwise.
    pub fn is_normalized(&self) -> bool {
        // Signature consists of R and S components respectively, each 32 bytes big.
        const OFFSET: usize = 32;

        let mut a: [u64; 4] = [0; 4];

        #[allow(clippy::needless_range_loop)]
        for i in 0..4 {
            let offset = OFFSET + (4 - i - 1) * 8;
            let bytes: [u8; 8] = self.0.as_slice()[offset..offset + 8].try_into().unwrap();
            a[i] = u64::from_be_bytes(bytes);
        }

        let [a0, a1, a2, a3] = a;

        if a0 == 0 && a1 == 0 && a2 == 0 && a3 == 0 {
            return false;
        }

        #[allow(clippy::if_same_then_else)]
        let is_lt_q_b = if a3 < 0xffffffffffffffff {
            true
        } else if a2 < 0xfffffffffffffffe {
            true
        } else if a2 > 0xfffffffffffffffe {
            false
        } else if a1 < 0xbaaedce6af48a03b {
            true
        } else if a1 > 0xbaaedce6af48a03b {
            false
        } else {
            a0 < 0xbfd25e8cd0364141
        };

        #[allow(clippy::if_same_then_else)]
        let is_s_lt_q_halved = if a3 < 0x7fffffffffffffff {
            true
        } else if a3 > 0x7fffffffffffffff {
            false
        } else if a2 < 0xffffffffffffffff {
            true
        } else if a1 < 0x5d576e7357a4501d {
            true
        } else if a1 > 0x5d576e7357a4501d {
            false
        } else {
            a0 <= 0xdfe92f46681b20a0
        };

        is_lt_q_b && is_s_lt_q_halved
    }
}

#[cfg(feature = "cosmwasm")]
impl Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        HexBinary::from(&self.0).fmt(f)
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&hex::encode(&self.0))
    }
}

#[cfg(feature = "cosmwasm")]
impl serde::Serialize for Signature {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        HexBinary::from(&self.0).serialize(serializer)
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl serde::Serialize for Signature {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "cosmwasm")]
impl<'de> serde::Deserialize<'de> for Signature {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let binary = HexBinary::deserialize(deserializer)?;
        let array = binary
            .to_array()
            .map_err(|_| serde::de::Error::custom("Incorrect size of signature data"))?;

        Ok(Signature(array))
    }
}

#[cfg(all(feature = "solana", not(feature = "cosmwasm")))]
impl<'de> serde::Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        let mut bytes = [0u8; 64];
        hex::decode_to_slice(s, &mut bytes as &mut [u8]).map_err(serde::de::Error::custom)?;

        Ok(Self(bytes))
    }
}
