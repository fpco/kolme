pub use rand::rngs::ThreadRng;

use std::{fmt::Display, str::FromStr};

/// Newtype wrapper around [k256::PublicKey] to provide consistent serialization.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PublicKey(k256::PublicKey);

#[derive(thiserror::Error, Debug)]
pub enum PublicKeyError {
    #[error("Unable to parse public key from bytes: {source}")]
    TryFromBytes { source: k256::elliptic_curve::Error },
    #[error("Unable to recover public key from message: {source}")]
    RecoverFromMessage { source: k256::ecdsa::Error },
    #[error("Invalid hex encoding for public key in {hex:?}: {source}")]
    InvalidHexEncoding {
        source: hex::FromHexError,
        hex: String,
    },
    #[error("Could not parse a public key from {bytes:?}: {source}")]
    FromBytes {
        source: k256::elliptic_curve::Error,
        bytes: Vec<u8>,
    },
}

impl PublicKey {
    pub fn as_bytes(&self) -> Box<[u8]> {
        self.0.to_sec1_bytes()
    }

    pub fn try_from_bytes(public_key: &[u8]) -> Result<Self, PublicKeyError> {
        k256::PublicKey::from_sec1_bytes(public_key)
            .map(PublicKey)
            .map_err(|source| PublicKeyError::TryFromBytes { source })
    }

    /// Validate a signature using a recovery ID, returning the public key used.
    pub fn recover_from_msg(
        payload: impl AsRef<[u8]>,
        signature: &Signature,
        recovery: RecoveryId,
    ) -> Result<Self, PublicKeyError> {
        k256::ecdsa::VerifyingKey::recover_from_msg(payload.as_ref(), &signature.0, recovery.0)
            .map(PublicKey::from)
            .map_err(|source| PublicKeyError::RecoverFromMessage { source })
    }

    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<Self, PublicKeyError> {
        let bytes = bytes.as_ref();
        k256::PublicKey::from_sec1_bytes(bytes)
            .map(Self)
            .map_err(|source| PublicKeyError::FromBytes {
                source,
                bytes: bytes.to_owned(),
            })
    }
}

impl From<k256::PublicKey> for PublicKey {
    fn from(value: k256::PublicKey) -> Self {
        PublicKey(value)
    }
}

impl From<k256::ecdsa::VerifyingKey> for PublicKey {
    fn from(value: k256::ecdsa::VerifyingKey) -> Self {
        k256::PublicKey::from(value).into()
    }
}

impl serde::Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "solana")]
impl borsh::ser::BorshSerialize for PublicKey {
    #[inline]
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
        use std::ops::Deref;

        // Serialize as an array instead of a Vec/slice because the chain version has that format.
        let arr: [u8; 33] = self.as_bytes().deref().try_into().unwrap();

        borsh::ser::BorshSerialize::serialize(&arr, writer)
    }
}

#[cfg(feature = "solana")]
impl borsh::de::BorshDeserialize for PublicKey {
    #[inline]
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> borsh::io::Result<Self> {
        use borsh::io::{Error, ErrorKind};

        // We are forced to use the internal API because using the [T; N] array impl returns
        // a "Not all bytes read" error even though the binary representation is correct...
        let bytes: [u8; 33] = u8::array_from_reader(reader)?.unwrap(); // This always returns Some
        match Self::from_bytes(bytes) {
            Ok(sig) => Ok(sig),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    }
}

impl FromStr for PublicKey {
    type Err = PublicKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = hex::decode(s).map_err(|source| PublicKeyError::InvalidHexEncoding {
            source,
            hex: s.to_owned(),
        })?;
        PublicKey::try_from_bytes(&bytes)
    }
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&hex::encode(self.0.to_sec1_bytes()))
    }
}

impl std::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::hash::Hash for PublicKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_sec1_bytes().hash(state)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SecretKeyError {
    #[error("Signing failed: {source}")]
    SigningFailed { source: k256::ecdsa::Error },
    #[error("Invalid hex when parsing secret key (contents redacted for privacy)")]
    InvalidHex,
    #[error("Invalid bytes when parsing a secret key: {source}")]
    InvalidBytes { source: k256::elliptic_curve::Error },
}

#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey(k256::SecretKey);

impl SecretKey {
    pub fn public_key(&self) -> PublicKey {
        self.0.public_key().into()
    }

    pub fn sign_recoverable(
        &self,
        msg: impl AsRef<[u8]>,
    ) -> Result<SignatureWithRecovery, SecretKeyError> {
        k256::ecdsa::SigningKey::from(&self.0)
            .sign_recoverable(msg.as_ref())
            .map(|(sig, rec)| SignatureWithRecovery {
                recid: rec.into(),
                sig: sig.into(),
            })
            .map_err(|source| SecretKeyError::SigningFailed { source })
    }

    /// Sign a payload that was already hashed beforehand.
    pub fn sign_prehash_recoverable(
        &self,
        msg: impl AsRef<[u8]>,
    ) -> Result<SignatureWithRecovery, SecretKeyError> {
        k256::ecdsa::SigningKey::from(&self.0)
            .sign_prehash_recoverable(msg.as_ref())
            .map(|(sig, rec)| SignatureWithRecovery {
                recid: rec.into(),
                sig: sig.into(),
            })
            .map_err(|source| SecretKeyError::SigningFailed { source })
    }

    pub fn random(rng: &mut rand::rngs::ThreadRng) -> Self {
        SecretKey(k256::SecretKey::random(rng))
    }

    pub fn from_hex(hex: &str) -> Result<Self, SecretKeyError> {
        let bytes = ::hex::decode(hex).map_err(|_| SecretKeyError::InvalidHex)?;
        k256::SecretKey::from_slice(&bytes)
            .map(SecretKey)
            .map_err(|source| SecretKeyError::InvalidBytes { source })
    }

    /// Reveal the secret key contents as a hex string
    ///
    /// This could be a Display impl, but intentionally making it
    /// more difficult so we don't accidentally leak secret keys.
    pub fn reveal_as_hex(&self) -> String {
        hex::encode(self.0.to_bytes())
    }
}

impl FromStr for SecretKey {
    type Err = SecretKeyError;

    fn from_str(s: &str) -> Result<Self, SecretKeyError> {
        SecretKey::from_hex(s)
    }
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("SecretKey(contents redacted)")
    }
}

mod sigerr {
    #[derive(thiserror::Error, Debug)]
    pub enum SignatureError {
        #[error("Invalid hex encoding for signature in {hex:?}: {source}")]
        InvalidHexEncoding {
            source: hex::FromHexError,
            hex: String,
        },
        #[error("Invalid signature in {bytes:?}: {source}")]
        InvalidSignature {
            source: k256::ecdsa::Error,
            bytes: Vec<u8>,
        },
    }
}

pub use sigerr::SignatureError;

use super::SignatureWithRecovery;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature(k256::ecdsa::Signature);

impl PartialOrd for Signature {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Signature {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.to_bytes().cmp(&other.0.to_bytes())
    }
}

impl Signature {
    pub fn as_bytes(&self) -> impl AsRef<[u8]> {
        self.0.to_bytes()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }

    pub fn from_hex(s: &str) -> Result<Signature, SignatureError> {
        let bytes = hex::decode(s).map_err(|source| SignatureError::InvalidHexEncoding {
            source,
            hex: s.to_owned(),
        })?;
        Self::from_slice(&bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Signature, SignatureError> {
        k256::ecdsa::Signature::from_slice(bytes)
            .map(Signature)
            .map_err(|source| SignatureError::InvalidSignature {
                source,
                bytes: bytes.to_owned(),
            })
    }
}

#[cfg(feature = "solana")]
impl borsh::ser::BorshSerialize for Signature {
    #[inline]
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
        use std::ops::Deref;

        // Serialize as an array instead of a Vec/slice because the chain version has that format.
        let arr: [u8; 64] = self.to_bytes().deref().try_into().unwrap();

        borsh::ser::BorshSerialize::serialize(&arr, writer)
    }
}

#[cfg(feature = "solana")]
impl borsh::de::BorshDeserialize for Signature {
    #[inline]
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> borsh::io::Result<Self> {
        use borsh::io::{Error, ErrorKind};

        // We are forced to use the internal API because using the [T; N] array impl returns
        // a "Not all bytes read" error even though the binary representation is correct...
        let bytes: [u8; 64] = u8::array_from_reader(reader)?.unwrap(); // This always returns Some
        match Self::from_slice(&bytes) {
            Ok(sig) => Ok(sig),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    }
}

impl From<k256::ecdsa::Signature> for Signature {
    fn from(sig: k256::ecdsa::Signature) -> Self {
        Signature(sig)
    }
}

impl Display for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum RecoveryIdError {
    #[error("Invalid byte provided for recovery ID: {byte}")]
    InvalidByte { byte: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecoveryId(k256::ecdsa::RecoveryId);
impl RecoveryId {
    pub fn to_byte(&self) -> u8 {
        self.0.to_byte()
    }

    pub fn from_byte(byte: u8) -> Result<Self, RecoveryIdError> {
        k256::ecdsa::RecoveryId::from_byte(byte)
            .map(RecoveryId)
            .ok_or(RecoveryIdError::InvalidByte { byte })
    }
}

impl Display for RecoveryId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.to_byte().fmt(f)
    }
}

impl serde::Serialize for RecoveryId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.to_byte().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for RecoveryId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let recid = u8::deserialize(deserializer)?;
        match k256::ecdsa::RecoveryId::from_byte(recid) {
            None => Err(serde::de::Error::custom("Invalid recovery ID provided")),
            Some(recid) => Ok(RecoveryId(recid)),
        }
    }
}

#[cfg(feature = "solana")]
impl borsh::ser::BorshSerialize for RecoveryId {
    #[inline]
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
        borsh::ser::BorshSerialize::serialize(&self.to_byte(), writer)
    }
}

#[cfg(feature = "solana")]
impl borsh::de::BorshDeserialize for RecoveryId {
    #[inline]
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> borsh::io::Result<Self> {
        use borsh::io::{Error, ErrorKind};

        let byte = u8::deserialize_reader(reader)?;

        match Self::from_byte(byte) {
            Ok(id) => Ok(id),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    }
}

impl From<k256::ecdsa::RecoveryId> for RecoveryId {
    fn from(recid: k256::ecdsa::RecoveryId) -> Self {
        RecoveryId(recid)
    }
}

impl SignatureWithRecovery {
    pub fn validate(&self, msg: &[u8]) -> Result<PublicKey, PublicKeyError> {
        PublicKey::recover_from_msg(msg, &self.sig, self.recid)
    }
}
