//! Zero-knowledge proof authentication for Kolme
//!
//! This crate provides ZKP-based social sign-on functionality, allowing users
//! to authenticate using their social media identities while preserving privacy
//! through zero-knowledge proofs.

pub mod circuits;
pub mod errors;
pub mod hash;
pub mod hash_simple;
pub mod keys;
pub mod prover;
pub mod recovery;
pub mod social;
pub mod verifier;

pub use errors::{ZkpAuthError, Result};
pub use prover::Prover;
pub use verifier::Verifier;

use serde::{Deserialize, Serialize};

/// Supported social platforms for ZKP authentication
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SocialPlatform {
    Twitter,
    GitHub,
    Discord,
    Google,
}

impl SocialPlatform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Twitter => "twitter",
            Self::GitHub => "github",
            Self::Discord => "discord",
            Self::Google => "google",
        }
    }
    
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "twitter" => Some(Self::Twitter),
            "github" => Some(Self::GitHub),
            "discord" => Some(Self::Discord),
            "google" => Some(Self::Google),
            _ => None,
        }
    }
}

/// A commitment to a social identity
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SocialIdentityCommitment {
    pub commitment: [u8; 32],
    pub platform: SocialPlatform,
}

/// A zero-knowledge proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkProof {
    pub proof_data: Vec<u8>,
    pub public_inputs: Vec<[u8; 32]>,
    pub platform: SocialPlatform,
}

/// Social identity information (never stored on-chain)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SocialIdentity {
    pub platform: SocialPlatform,
    pub user_id: String,
    pub username: Option<String>,
    pub email: Option<String>,
    pub verified: bool,
    pub attributes: std::collections::HashMap<String, String>,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_platform_conversion() {
        assert_eq!(SocialPlatform::from_str("twitter"), Some(SocialPlatform::Twitter));
        assert_eq!(SocialPlatform::from_str("GITHUB"), Some(SocialPlatform::GitHub));
        assert_eq!(SocialPlatform::from_str("invalid"), None);
        
        assert_eq!(SocialPlatform::Twitter.as_str(), "twitter");
    }
}