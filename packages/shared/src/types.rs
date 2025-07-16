use std::{collections::BTreeSet, fmt::Display, num::TryFromIntError};

use crate::cryptography::{PublicKey, SignatureWithRecovery};
#[cfg(feature = "realcryptography")]
use crate::cryptography::{SecretKey, SecretKeyError};

/// Monotonically increasing identifier for actions sent to a bridge contract.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct BridgeActionId(pub u64);
impl BridgeActionId {
    pub fn start() -> Self {
        BridgeActionId(0)
    }

    pub fn next(self) -> BridgeActionId {
        BridgeActionId(self.0 + 1)
    }

    pub fn increment(&mut self) {
        self.0 += 1;
    }
}

impl Display for BridgeActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Monotonically increasing identifier for events coming from a bridge contract.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct BridgeEventId(pub u64);
impl BridgeEventId {
    pub fn start() -> BridgeEventId {
        BridgeEventId(0)
    }

    pub fn try_from_i64(id: i64) -> Result<Self, TryFromIntError> {
        id.try_into().map(BridgeEventId)
    }

    pub fn next(self) -> BridgeEventId {
        BridgeEventId(self.0 + 1)
    }

    pub fn increment(&mut self) {
        self.0 += 1;
    }

    pub fn prev(self) -> Option<BridgeEventId> {
        self.0.checked_sub(1).map(BridgeEventId)
    }
}

impl Display for BridgeEventId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "cosmwasm")]
mod cw_impls {
    use super::*;
    use cw_storage_plus::{KeyDeserialize, PrimaryKey};

    impl KeyDeserialize for BridgeEventId {
        type Output = Self;

        const KEY_ELEMS: u16 = 1;

        fn from_vec(value: Vec<u8>) -> cosmwasm_std::StdResult<Self::Output> {
            <u64 as KeyDeserialize>::from_vec(value).map(Self)
        }
    }

    impl PrimaryKey<'_> for BridgeEventId {
        type Prefix = ();
        type SubPrefix = ();
        type Suffix = Self;
        type SuperSuffix = Self;

        fn key(&self) -> Vec<cw_storage_plus::Key> {
            self.0.key()
        }
    }
}

/// A binary value representing a SHA256 hash.
#[derive(PartialEq, Eq, Hash, Clone, Copy, PartialOrd, Ord)]
pub struct Sha256Hash([u8; 32]);

#[derive(thiserror::Error, Debug)]
pub enum Sha256HashError {
    #[error("Wrong byte count for a SHA256 hash. Expected 32, received {actual}.")]
    WrongByteCount { actual: usize },
}

impl Sha256Hash {
    #[cfg(feature = "realcryptography")]
    pub fn hash(input: impl AsRef<[u8]>) -> Self {
        Sha256Hash(<k256::sha2::Sha256 as k256::sha2::Digest>::digest(input.as_ref()).into())
    }

    pub fn from_hash(state: &[u8]) -> Result<Self, Sha256HashError> {
        state
            .try_into()
            .map(Sha256Hash)
            .map_err(|_| Sha256HashError::WrongByteCount {
                actual: state.len(),
            })
    }

    pub fn as_array(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_array(array: [u8; 32]) -> Self {
        Sha256Hash(array)
    }
}

#[cfg(feature = "realcryptography")]
impl Display for Sha256Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0.as_slice()))
    }
}

#[cfg(feature = "realcryptography")]
impl std::str::FromStr for Sha256Hash {
    type Err = std::convert::Infallible; // FIXME use a real error

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Sha256Hash::from_hash(&hex::decode(s).unwrap()).unwrap())
    }
}

#[cfg(feature = "realcryptography")]
impl std::fmt::Debug for Sha256Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(feature = "realcryptography")]
impl serde::Serialize for Sha256Hash {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0.as_slice()))
    }
}

#[cfg(feature = "realcryptography")]
impl<'de> serde::Deserialize<'de> for Sha256Hash {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(D::Error::custom)?;
        Sha256Hash::from_hash(&bytes).map_err(D::Error::custom)
    }
}

#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorType {
    Listener,
    Processor,
    Approver,
}

impl Display for ValidatorType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            ValidatorType::Listener => "listener",
            ValidatorType::Processor => "processor",
            ValidatorType::Approver => "approver",
        })
    }
}

/// The payload for self-replacing.
///
/// We separate this to its own type so that we can
/// pass along this message with its signature to the contracts.
#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct SelfReplace {
    pub validator_type: ValidatorType,
    pub replacement: PublicKey,
}

/// Definition of the validator set for a chain.
#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ValidatorSet {
    pub processor: PublicKey,
    pub listeners: BTreeSet<PublicKey>,
    pub needed_listeners: u16,
    pub approvers: BTreeSet<PublicKey>,
    pub needed_approvers: u16,
}

#[derive(thiserror::Error, Debug)]
pub enum ValidatorSetError {
    #[error("This operation requires a validator to perform it, but {key} is not part of the validator set.")]
    NotAValidator { key: PublicKey },
    #[error("Too many keys provided in validator set: {source}")]
    TooManyKeys { source: TryFromIntError },
    #[error("No listeners provided in a validator set")]
    NoListeners,
    #[error(
        "Validator set specified {needed} for listener quorum, but only found {provided} keys."
    )]
    InvalidNeededListeners { provided: u16, needed: u16 },
    #[error("No approvers provided in a validator set")]
    NoApprovers,
    #[error(
        "Validator set specified {needed} for approver quorum, but only found {provided} keys."
    )]
    InvalidNeededApprovers { provided: u16, needed: u16 },
}

impl ValidatorSet {
    pub fn validate(&self) -> Result<(), ValidatorSetError> {
        let listeners = u16::try_from(self.listeners.len())
            .map_err(|source| ValidatorSetError::TooManyKeys { source })?;
        let approvers = u16::try_from(self.approvers.len())
            .map_err(|source| ValidatorSetError::TooManyKeys { source })?;
        if listeners == 0 {
            return Err(ValidatorSetError::NoListeners);
        }
        if listeners < self.needed_listeners || self.needed_listeners == 0 {
            return Err(ValidatorSetError::InvalidNeededListeners {
                provided: listeners,
                needed: self.needed_listeners,
            });
        }
        if approvers == 0 {
            return Err(ValidatorSetError::NoApprovers);
        }
        if approvers < self.needed_approvers || self.needed_approvers == 0 {
            return Err(ValidatorSetError::InvalidNeededApprovers {
                provided: approvers,
                needed: self.needed_approvers,
            });
        }
        Ok(())
    }

    /// Ensure that the given public key is a member of one of the validator sets.
    pub fn ensure_is_validator(&self, key: PublicKey) -> Result<(), ValidatorSetError> {
        if key == self.processor || self.listeners.contains(&key) || self.approvers.contains(&key) {
            Ok(())
        } else {
            Err(ValidatorSetError::NotAValidator { key })
        }
    }
}

#[cfg_attr(
    feature = "solana",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize)
)]
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct KeyRegistration {
    pub signature: SignatureWithRecovery,
    pub key: PublicKey,
}

#[cfg(feature = "realcryptography")]
impl KeyRegistration {
    pub fn cosmos(address: &str, key: &SecretKey) -> Result<Self, SecretKeyError> {
        Ok(Self {
            signature: key.sign_recoverable(address)?,
            key: key.public_key(),
        })
    }

    pub fn solana(pubkey: [u8; 32], key: &SecretKey) -> Result<Self, SecretKeyError> {
        Ok(Self {
            signature: key.sign_prehash_recoverable(pubkey)?,
            key: key.public_key(),
        })
    }
}
