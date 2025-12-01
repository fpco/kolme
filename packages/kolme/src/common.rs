/// Common helper functions and utilities.
use std::{fmt::Display, sync::OnceLock};

use crate::*;

use tracing::Level;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize the logging system.
///
/// This leverages the tracing crate. If verbose is enabled,
/// debug messages for both the Kolme crate itself, and if provided
/// the local crate, will be logged.
pub fn init_logger(verbose: bool, local_crate_name: Option<&str>) {
    static LOGGER_SETUP: OnceLock<()> = OnceLock::new();
    LOGGER_SETUP.get_or_init(|| {
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
    });
}
/// Tagged, consistent-binary JSON
///
/// JSON data in a consistent format, serialized as a JSON string, with the parsed value available as well.
///
/// Equality is based on serialized representation only.
#[derive(Clone)]
pub struct TaggedJson<T> {
    serialized: String,
    hash: Sha256Hash,
    value: T,
}

impl<T> MerkleSerializeRaw for TaggedJson<T> {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut MerkleSerializer,
    ) -> Result<(), MerkleSerialError> {
        serializer.store(&self.serialized)
    }
}

impl<T: serde::de::DeserializeOwned> MerkleDeserializeRaw for TaggedJson<T> {
    fn merkle_deserialize_raw(
        deserializer: &mut MerkleDeserializer,
    ) -> Result<Self, MerkleSerialError> {
        let serialized: String = deserializer.load()?;
        Self::try_from_string(serialized).map_err(MerkleSerialError::custom)
    }
}

/// Wrap up a [TaggedJson] with a signature and recovery information.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
#[serde(bound(serialize = "", deserialize = "T: serde::de::DeserializeOwned"))]
pub struct SignedTaggedJson<T> {
    pub message: TaggedJson<T>,
    pub signature: Signature,
    pub recovery_id: RecoveryId,
}

impl<T> SignedTaggedJson<T> {
    pub fn signature_with_recovery(&self) -> SignatureWithRecovery {
        SignatureWithRecovery {
            recid: self.recovery_id,
            sig: self.signature,
        }
    }
}

impl<T> MerkleSerialize for SignedTaggedJson<T> {
    fn merkle_serialize(&self, serializer: &mut MerkleSerializer) -> Result<(), MerkleSerialError> {
        let Self {
            message,
            signature,
            recovery_id,
        } = self;
        serializer.store(message)?;
        serializer.store(signature)?;
        serializer.store(recovery_id)?;
        Ok(())
    }
}

impl<T: serde::de::DeserializeOwned> MerkleDeserialize for SignedTaggedJson<T> {
    fn merkle_deserialize(
        deserializer: &mut MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        Ok(Self {
            message: deserializer.load()?,
            signature: deserializer.load()?,
            recovery_id: deserializer.load()?,
        })
    }
}

impl<T> SignedTaggedJson<T> {
    pub fn verify_signature(&self) -> Result<PublicKey> {
        PublicKey::recover_from_msg(self.message.as_bytes(), &self.signature, self.recovery_id)
            .map_err(anyhow::Error::from)
    }

    pub(crate) fn message_hash(&self) -> Sha256Hash {
        self.message.hash
    }
}

impl<T> PartialEq for SignedTaggedJson<T> {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
            && self.signature == other.signature
            && self.recovery_id == other.recovery_id
    }
}

impl<T> Eq for SignedTaggedJson<T> {}

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
        let serialized = serde_json::to_string(&value)?;
        let hash = Sha256Hash::hash(&serialized);
        Ok(TaggedJson {
            serialized,
            hash,
            value,
        })
    }

    pub fn sign(self, key: &SecretKey) -> Result<SignedTaggedJson<T>> {
        let SignatureWithRecovery { recid, sig } = key.sign_recoverable(self.as_bytes())?;
        Ok(SignedTaggedJson {
            message: self,
            signature: sig,
            recovery_id: recid,
        })
    }
}

impl<T: serde::de::DeserializeOwned> TaggedJson<T> {
    pub fn try_from_string(serialized: String) -> serde_json::Result<Self> {
        let value = serde_json::from_str(&serialized)?;
        let hash = Sha256Hash::hash(&serialized);
        Ok(TaggedJson {
            value,
            serialized,
            hash,
        })
    }
}

impl<T> TaggedJson<T> {
    /// Use a combo of the serialized and value format for input.
    ///
    /// Note that this function does no checking to confirm that these values match.
    pub fn from_pair(value: T, serialized: String) -> Self {
        let hash = Sha256Hash::hash(&serialized);
        TaggedJson {
            serialized,
            value,
            hash,
        }
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
        serializer.serialize_str(&self.serialized)
    }
}

impl<'de, T: serde::de::DeserializeOwned> serde::Deserialize<'de> for TaggedJson<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let serialized = String::deserialize(deserializer)?;
        Self::try_from_string(serialized).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

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
        assert_eq!(Sha256Hash::from_array(*hash1.as_array()), hash1);
        Sha256Hash::from_hash(b"invalid input").unwrap_err();
    }

    #[test]
    fn round_trip_public_key() {
        let secret = SecretKey::random();
        let public = secret.public_key();

        assert_eq!(
            serde_json::to_value(public).unwrap(),
            serde_json::Value::String(public.to_string())
        );

        let json = serde_json::to_vec(&public).unwrap();
        let json_parsed = serde_json::from_slice::<PublicKey>(&json).unwrap();
        assert_eq!(public, json_parsed);

        let s = public.to_string();
        let display_parsed = s.parse().unwrap();
        assert_eq!(public, display_parsed);
    }

    #[test]
    fn round_trip_secret_key() {
        let secret = SecretKey::random();
        let s = secret.reveal_as_hex();
        let secret2 = SecretKey::from_str(&s).unwrap();
        assert_eq!(secret, secret2);
        // Make sure the Debug impl doesn't leak key data
        assert_ne!(format!("{secret:?}"), s);
    }

    #[test]
    fn parse_secret_key() {
        let secret =
            SecretKey::from_hex("658c3528422eb527b4c108b8f6d1e5f629543c304ea49cf608c67794424291c4")
                .unwrap();
        let public = secret.public_key();
        assert_eq!(
            public.to_string(),
            "0264eb26609d15e709227b9ddc46c11a738b210bb237949aa86d7d490a35ae0f0a"
        );
    }
}
