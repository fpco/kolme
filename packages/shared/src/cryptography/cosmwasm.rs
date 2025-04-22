use std::fmt::Display;

use cosmwasm_std::HexBinary;

#[derive(thiserror::Error, Debug)]
pub enum PublicKeyError {}

#[derive(Debug, PartialEq, Eq, Hash, Clone, PartialOrd, Ord, Copy)]
pub struct PublicKey([u8; 33]);

impl PublicKey {
    pub fn from_bytes(bytes: [u8; 33]) -> Result<Self, PublicKeyError> {
        Ok(PublicKey(bytes))
    }
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        HexBinary::from(&self.0).fmt(f)
    }
}

impl serde::Serialize for PublicKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        HexBinary::from(&self.0).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for PublicKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let binary = HexBinary::deserialize(deserializer)?;
        let array = binary
            .to_array()
            .map_err(|_| serde::de::Error::custom("Incorrect size of public key data"))?;
        Ok(PublicKey(array))
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
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

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct Signature([u8; 64]);

impl Signature {
    pub fn to_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        HexBinary::from(&self.0).fmt(f)
    }
}

impl serde::Serialize for Signature {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        HexBinary::from(&self.0).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Signature {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let binary = HexBinary::deserialize(deserializer)?;
        let array = binary
            .to_array()
            .map_err(|_| serde::de::Error::custom("Incorrect size of signature data"))?;
        Ok(Signature(array))
    }
}
