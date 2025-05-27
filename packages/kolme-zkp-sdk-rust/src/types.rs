use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};

/// Re-export types from kolme-zkp-auth for convenience
pub use kolme_zkp_auth::{SocialPlatform, SocialIdentity, SocialIdentityCommitment, ZkProof};

/// Configuration for the Kolme ZKP SDK
#[derive(Debug, Clone)]
pub struct KolmeZkpConfig {
    /// Base URL of the Kolme API server
    pub api_url: String,
    /// Optional WebSocket URL for real-time updates
    pub websocket_url: Option<String>,
    /// Request timeout in seconds (default: 30)
    pub timeout: Option<u64>,
    /// Custom HTTP headers to include with requests
    pub headers: HashMap<String, String>,
    /// Whether to verify SSL certificates (default: true)
    pub verify_ssl: bool,
}

impl Default for KolmeZkpConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8080".to_string(),
            websocket_url: None,
            timeout: Some(30),
            headers: HashMap::new(),
            verify_ssl: true,
        }
    }
}

impl KolmeZkpConfig {
    /// Create a new configuration with the specified API URL
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            ..Default::default()
        }
    }

    /// Set the WebSocket URL for real-time updates
    pub fn with_websocket_url(mut self, url: impl Into<String>) -> Self {
        self.websocket_url = Some(url.into());
        self
    }

    /// Set the request timeout in seconds
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Add a custom HTTP header
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Disable SSL certificate verification (for testing only)
    pub fn with_insecure_ssl(mut self) -> Self {
        self.verify_ssl = false;
        self
    }
}

/// Public key for cryptographic operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    /// Create a new public key from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Get the bytes of the public key
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        Ok(Self(array))
    }
}

/// Digital signature for authentication
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature(pub [u8; 64]);

impl Signature {
    /// Create a new signature from bytes
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Get the bytes of the signature
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    pub fn from_hex(hex: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex)?;
        if bytes.len() != 64 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut array = [0u8; 64];
        array.copy_from_slice(&bytes);
        Ok(Self(array))
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex_string = String::deserialize(deserializer)?;
        Self::from_hex(&hex_string).map_err(serde::de::Error::custom)
    }
}

/// OAuth initialization request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitAuthRequest {
    /// Social platform to authenticate with
    pub platform: SocialPlatform,
    /// OAuth redirect URI for callback
    pub redirect_uri: String,
    /// Optional public key for cryptographic operations
    pub public_key: Option<PublicKey>,
}

/// OAuth initialization response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitAuthResponse {
    /// URL for user to visit to complete OAuth
    pub auth_url: String,
    /// OAuth state parameter for security
    pub state: String,
}

/// OAuth callback request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackRequest {
    /// OAuth authorization code from callback
    pub code: String,
    /// OAuth state parameter for verification
    pub state: String,
}

/// OAuth callback response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackResponse {
    /// Whether the OAuth flow was successful
    pub success: bool,
    /// Authenticated user's social identity
    pub identity: Option<SocialIdentity>,
    /// Session ID for subsequent operations
    pub session_id: Option<String>,
}

/// Proof generation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveIdentityRequest {
    /// Session ID for the authenticated user
    pub session_id: String,
    /// Cryptographic signature for proof generation
    pub signature: Signature,
}

/// Proof generation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProveIdentityResponse {
    /// Identity commitment for the proof
    pub commitment: SocialIdentityCommitment,
    /// Generated zero-knowledge proof
    pub proof: ZkProof,
}

/// Proof verification request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyProofRequest {
    /// Identity commitment to verify
    pub commitment: SocialIdentityCommitment,
    /// Zero-knowledge proof to verify
    pub proof: ZkProof,
    /// Social platform for verification context
    pub platform: SocialPlatform,
}

/// Proof verification response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyProofResponse {
    /// Whether the proof is valid
    pub valid: bool,
    /// Platform that was verified (if successful)
    pub platform: Option<SocialPlatform>,
}

/// Session status query parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusQuery {
    /// Session ID to check status for
    pub session_id: String,
}

/// Session status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Whether the session is still authenticated
    pub authenticated: bool,
    /// User's social identity (if authenticated)
    pub identity: Option<SocialIdentity>,
    /// Current identity commitment (if available)
    pub commitment: Option<SocialIdentityCommitment>,
}

/// Authentication session information
#[derive(Debug, Clone)]
pub struct AuthSession {
    /// Unique session identifier
    pub session_id: String,
    /// Authenticated user's social identity
    pub identity: SocialIdentity,
    /// Optional identity commitment (if proof generated)
    pub commitment: Option<SocialIdentityCommitment>,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// When the session was last accessed
    pub last_accessed: DateTime<Utc>,
}

impl AuthSession {
    /// Create a new authentication session
    pub fn new(session_id: String, identity: SocialIdentity) -> Self {
        let now = Utc::now();
        Self {
            session_id,
            identity,
            commitment: None,
            created_at: now,
            last_accessed: now,
        }
    }

    /// Update the last accessed time
    pub fn touch(&mut self) {
        self.last_accessed = Utc::now();
    }

    /// Check if the session is expired (older than 24 hours)
    pub fn is_expired(&self) -> bool {
        let now = Utc::now();
        now.signed_duration_since(self.created_at).num_hours() >= 24
    }
}

/// Events emitted by the SDK
#[derive(Debug, Clone)]
pub enum KolmeZkpEvent {
    /// Authentication flow started
    AuthStarted {
        /// Platform being authenticated with
        platform: SocialPlatform
    },
    /// Authentication completed successfully
    AuthCompleted {
        /// Authenticated user's identity
        identity: SocialIdentity,
        /// Session ID for the authenticated user
        session_id: Option<String>
    },
    /// Authentication failed
    AuthFailed {
        /// Error message describing the failure
        error: String
    },
    /// ZKP proof generated
    ProofGenerated {
        /// Identity commitment for the proof
        commitment: SocialIdentityCommitment,
        /// Generated zero-knowledge proof
        proof: ZkProof
    },
    /// ZKP proof verified
    ProofVerified {
        /// Whether the proof is valid
        valid: bool,
        /// Platform that was verified
        platform: Option<SocialPlatform>
    },
    /// Session expired
    SessionExpired {
        /// ID of the expired session
        session_id: String
    },
    /// General error occurred
    Error {
        /// Error message
        error: String,
        /// Optional additional error details
        details: Option<String>
    },
}

/// Event listener trait for handling SDK events
#[async_trait::async_trait]
pub trait EventListener: Send + Sync {
    /// Handle an SDK event
    async fn on_event(&self, event: KolmeZkpEvent);
}

/// API error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Error message from the API
    pub error: String,
    /// Optional additional error details
    pub details: Option<serde_json::Value>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for ApiError {}
