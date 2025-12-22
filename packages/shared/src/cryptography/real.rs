pub use rand::rngs::ThreadRng;

use chacha20poly1305::{
    aead::{AeadInPlace, KeyInit},
    ChaCha20Poly1305, Key, Nonce, Tag,
};
use k256::ecdh::{diffie_hellman, EphemeralSecret, SharedSecret};
use rand::{CryptoRng, RngCore};
use sha2::Sha256;
use std::{fmt::Display, str::FromStr};

const KEY_DERIVATION_INFO: &[u8] = b"kolme/chacha20poly1305-v1";

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

    /// Encrypt data so it can be decrypted by the corresponding [SecretKey].
    pub fn encrypt(
        &self,
        plaintext: impl AsRef<[u8]>,
    ) -> Result<EncryptedMessage, EncryptionError> {
        self.encrypt_with(&mut rand::thread_rng(), plaintext)
    }

    pub fn encrypt_with(
        &self,
        rng: &mut (impl CryptoRng + RngCore),
        plaintext: impl AsRef<[u8]>,
    ) -> Result<EncryptedMessage, EncryptionError> {
        let ephemeral_secret = EphemeralSecret::random(rng);
        let ephemeral_public_key: PublicKey = k256::PublicKey::from(&ephemeral_secret).into();
        let shared_secret = ephemeral_secret.diffie_hellman(&self.0);
        let key = derive_encryption_key(&shared_secret)?;

        let mut nonce_bytes = [0u8; 12];
        rng.fill_bytes(&mut nonce_bytes);

        let cipher = ChaCha20Poly1305::new(&key);
        let mut buffer = plaintext.as_ref().to_vec();
        let tag = cipher
            .encrypt_in_place_detached(Nonce::from_slice(&nonce_bytes), &[], &mut buffer)
            .map_err(|_| EncryptionError::EncryptionFailed)?;

        Ok(EncryptedMessage {
            ephemeral_public_key,
            nonce: nonce_bytes,
            ciphertext: buffer,
            tag: tag.into(),
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

#[derive(thiserror::Error, Debug)]
pub enum EncryptionError {
    #[error("Key derivation failed")]
    KeyDerivationFailed,
    #[error("Encryption failed")]
    EncryptionFailed,
    #[error("Decryption failed")]
    DecryptionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EncryptedMessage {
    pub ephemeral_public_key: PublicKey,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
    pub tag: [u8; 16],
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

    /// Produce a random secret key using [rand::thread_rng]
    pub fn random() -> Self {
        Self::random_with(&mut rand::thread_rng())
    }

    pub fn random_with(rng: &mut rand::rngs::ThreadRng) -> Self {
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

    pub fn decrypt(&self, message: &EncryptedMessage) -> Result<Vec<u8>, EncryptionError> {
        let shared_secret = diffie_hellman(
            self.0.to_nonzero_scalar(),
            message.ephemeral_public_key.0.as_affine(),
        );
        let key = derive_encryption_key(&shared_secret)?;

        let cipher = ChaCha20Poly1305::new(&key);
        let mut buffer = message.ciphertext.clone();
        let tag = Tag::from_slice(&message.tag);
        cipher
            .decrypt_in_place_detached(Nonce::from_slice(&message.nonce), &[], &mut buffer, tag)
            .map_err(|_| EncryptionError::DecryptionFailed)?;

        Ok(buffer)
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

fn derive_encryption_key(shared_secret: &SharedSecret) -> Result<Key, EncryptionError> {
    let hkdf = shared_secret.extract::<Sha256>(None);
    let mut okm = [0u8; 32];
    hkdf.expand(KEY_DERIVATION_INFO, &mut okm)
        .map_err(|_| EncryptionError::KeyDerivationFailed)?;

    Ok(Key::clone_from_slice(&okm))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let receiver = SecretKey::random();
        let plaintext = b"hello world";

        let encrypted = receiver.public_key().encrypt(plaintext).unwrap();
        let decrypted = receiver.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_is_randomized() {
        let receiver = SecretKey::random();
        let plaintext = b"stable text";

        let first = receiver.public_key().encrypt(plaintext).unwrap();
        let second = receiver.public_key().encrypt(plaintext).unwrap();

        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.ciphertext, second.ciphertext);
        assert_ne!(first.ephemeral_public_key, second.ephemeral_public_key);
    }

    #[test]
    fn tampering_fails() {
        let receiver = SecretKey::random();
        let mut encrypted = receiver.public_key().encrypt(b"super secret").unwrap();

        encrypted.ciphertext[0] ^= 0b0000_0001;

        assert!(receiver.decrypt(&encrypted).is_err());
    }
}
