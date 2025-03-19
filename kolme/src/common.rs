/// Common helper functions and utilities.
use std::{borrow::Cow, fmt::Display};

use crate::*;

use base64::{prelude::BASE64_STANDARD, Engine};
use k256::{
    ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey},
    elliptic_curve::generic_array::GenericArray,
    sha2::{digest::OutputSizeUser, Digest, Sha256},
};
use sqlx::{
    sqlite::{SqliteArgumentValue, SqliteValueRef},
    Decode, Encode, Sqlite,
};
use tracing::Level;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize the logging system.
///
/// This leverages the tracing crate. If verbose is enabled,
/// debug messages for both the Kolme crate itself, and if provided
/// the local crate, will be logged.
pub fn init_logger(verbose: bool, local_crate_name: Option<&str>) {
    let env_filter = if verbose {
        match local_crate_name {
            None => format!("{}=debug,info", env!("CARGO_CRATE_NAME")),
            Some(name) => format!("{}=debug,{name}=debug,info", env!("CARGO_CRATE_NAME")),
        }
        .parse()
        .unwrap()
    } else {
        EnvFilter::from_default_env().add_directive(Level::INFO.into())
    };

    tracing_subscriber::registry()
        .with(
            fmt::Layer::default()
                .log_internal_errors(true)
                .and_then(env_filter),
        )
        .init();
    tracing::info!("Initialized Logging");
}
/// Tagged, consistent-binary JSON
///
/// JSON data in a consistent format, serialized as Base64, with the parsed value available as well.
///
/// Equality is based on serialized representation only.
#[derive(Clone)]
pub struct TaggedJson<T> {
    serialized: String,
    value: T,
}

/// Wrap up a [TaggedJson] with a signature and recovery information.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(bound(serialize = "", deserialize = "T: serde::de::DeserializeOwned"))]
pub struct SignedTaggedJson<T> {
    pub message: TaggedJson<T>,
    pub signature: Signature,
    #[serde(with = "recovery")]
    pub recovery_id: RecoveryId,
}

impl<T> SignedTaggedJson<T> {
    pub fn verify_signature(&self) -> Result<PublicKey> {
        VerifyingKey::recover_from_msg(self.message.as_bytes(), &self.signature, self.recovery_id)
            .map(PublicKey::from)
            .map_err(anyhow::Error::from)
    }

    pub(crate) fn message_hash(&self) -> Sha256Hash {
        Sha256Hash::hash(self.message.as_bytes())
    }
}

mod recovery {
    use k256::ecdsa::RecoveryId;
    use serde::{Deserialize, Serialize};

    pub(super) fn serialize<S: serde::Serializer>(
        id: &RecoveryId,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        id.to_byte().serialize(s)
    }

    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<RecoveryId, D::Error> {
        let byte = u8::deserialize(d)?;
        RecoveryId::from_byte(byte).ok_or_else(|| serde::de::Error::custom("Invalid recovery ID"))
    }
}

impl<T> PartialEq for TaggedJson<T> {
    fn eq(&self, other: &Self) -> bool {
        self.serialized == other.serialized
    }
}
impl<T> Eq for TaggedJson<T> {}

impl<T> Display for TaggedJson<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.serialized)
    }
}

impl<T> std::fmt::Debug for TaggedJson<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.serialized)
    }
}

impl<T: serde::Serialize> TaggedJson<T> {
    pub fn new(value: T) -> Result<Self> {
        Ok(TaggedJson {
            serialized: serde_json::to_string(&value)?,
            value,
        })
    }

    pub fn sign(self, key: &k256::SecretKey) -> Result<SignedTaggedJson<T>> {
        let (signature, recovery_id) =
            SigningKey::from(key.clone()).sign_recoverable(self.as_bytes())?;
        Ok(SignedTaggedJson {
            message: self,
            signature,
            recovery_id,
        })
    }
}

impl<T: serde::de::DeserializeOwned> TaggedJson<T> {
    pub fn try_from_string(serialized: String) -> Result<Self> {
        let value = serde_json::from_str(&serialized)?;
        Ok(TaggedJson { value, serialized })
    }
}

impl<T> TaggedJson<T> {
    /// Use a combo of the serialized and value format for input.
    ///
    /// Note that this function does no checking to confirm that these values match.
    pub fn from_pair(value: T, serialized: String) -> Self {
        TaggedJson { serialized, value }
    }

    pub fn into_inner(self) -> T {
        self.value
    }

    pub fn as_inner(&self) -> &T {
        &self.value
    }

    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.serialized.as_bytes()
    }
}

impl<T> serde::Serialize for TaggedJson<T> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&BASE64_STANDARD.encode(&self.serialized))
    }
}

impl<'de, T: serde::de::DeserializeOwned> serde::Deserialize<'de> for TaggedJson<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let base64 = String::deserialize(deserializer)?;
        let bytes = BASE64_STANDARD.decode(&base64).map_err(D::Error::custom)?;
        let serialized = String::from_utf8(bytes).map_err(D::Error::custom)?;
        let value = serde_json::from_str::<T>(&serialized).map_err(D::Error::custom)?;
        Ok(TaggedJson { serialized, value })
    }
}

/// A binary value representing a SHA256 hash.
#[derive(PartialEq, Eq, Debug, Hash, Clone, Copy)]
pub struct Sha256Hash(pub GenericArray<u8, <Sha256 as OutputSizeUser>::OutputSize>);

impl Sha256Hash {
    pub fn hash(input: impl AsRef<[u8]>) -> Self {
        Sha256Hash(Sha256::digest(input.as_ref()))
    }

    pub fn from_hash(state: &[u8]) -> Result<Self> {
        // FIXME instead of hard-coding 32, use OutputSize correctly
        if state.len() == 32 {
            Ok(Sha256Hash(*GenericArray::from_slice(state)))
        } else {
            Err(anyhow::anyhow!(
                "Sha256Hash::from_hash: wrong length of {}",
                state.len()
            ))
        }
    }
}

impl serde::Serialize for Sha256Hash {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&hex::encode(self.0.as_slice()))
    }
}

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

impl sqlx::Type<sqlx::Sqlite> for Sha256Hash {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <&[u8] as sqlx::Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <&[u8] as sqlx::Type<Sqlite>>::compatible(ty)
    }
}

impl Encode<'_, sqlx::Sqlite> for Sha256Hash {
    fn encode_by_ref(
        &self,
        args: &mut Vec<SqliteArgumentValue>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        args.push(SqliteArgumentValue::Blob(Cow::Owned(self.0.to_vec())));

        Ok(sqlx::encode::IsNull::No)
    }
}

impl Decode<'_, sqlx::Sqlite> for Sha256Hash {
    fn decode(value: SqliteValueRef<'_>) -> Result<Self, sqlx::error::BoxDynError> {
        let vec: Vec<u8> = Decode::<sqlx::Sqlite>::decode(value)?;
        Sha256Hash::from_hash(&vec).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tagged_json() {
        #[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
        struct Test {
            field1: bool,
            field2: u32,
            field3: String,
        }
        let t = Test {
            field1: false,
            field2: 23,
            field3: "hello there".to_owned(),
        };

        let tagged1 = TaggedJson::new(t.clone()).unwrap();
        assert_eq!(t, tagged1.value);
        assert_eq!(serde_json::to_string(&t).unwrap(), tagged1.serialized);

        let pretty = serde_json::to_string_pretty(&t).unwrap();
        let tagged2 = TaggedJson::try_from_string(pretty.clone()).unwrap();
        assert_eq!(tagged1.value, tagged2.value);
        assert_eq!(pretty, tagged2.serialized);
        assert_ne!(tagged1.serialized, tagged2.serialized);

        let tagged1_serialized = serde_json::to_string(&tagged1).unwrap();
        assert_eq!(tagged1, serde_json::from_str(&tagged1_serialized).unwrap());
        let tagged2_serialized = serde_json::to_string(&tagged2).unwrap();
        assert_eq!(tagged2, serde_json::from_str(&tagged2_serialized).unwrap());
    }

    #[test]
    fn test_sha256hash() {
        let input = b"this is just some sample input";
        let hash1 = Sha256Hash::hash(input);
        let serialized = serde_json::to_string(&hash1).unwrap();
        let hash2 = serde_json::from_str(&serialized).unwrap();
        assert_eq!(hash1, hash2);
        assert_eq!(Sha256Hash::from_hash(&hash1.0).unwrap(), hash1);
        Sha256Hash::from_hash(b"invalid input").unwrap_err();
    }
}
