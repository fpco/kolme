use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub enum ZkpAuthError {
    #[snafu(display("Invalid social platform: {platform}"))]
    InvalidPlatform { platform: String },

    #[snafu(display("Proof generation failed: {reason}"))]
    ProofGenerationFailed { reason: String },

    #[snafu(display("Proof verification failed"))]
    ProofVerificationFailed,

    #[snafu(display("Invalid commitment"))]
    InvalidCommitment,

    #[snafu(display("OAuth validation failed: {reason}"))]
    OAuthValidationFailed { reason: String },

    #[snafu(display("Circuit synthesis error: {reason}"))]
    CircuitSynthesisError { reason: String },

    #[snafu(display("Trusted setup not initialized"))]
    TrustedSetupNotInitialized,

    #[snafu(display("Serialization error: {reason}"))]
    SerializationError { reason: String },

    #[snafu(display("Invalid proof format"))]
    InvalidProofFormat,

    #[snafu(display("Social provider error: {provider} - {reason}"))]
    SocialProviderError { provider: String, reason: String },

    #[snafu(display("Token validation failed: {reason}"))]
    TokenValidation { reason: String },

    #[snafu(display("Invalid token"))]
    InvalidToken,

    #[snafu(display("Invalid code"))]
    InvalidCode,

    #[snafu(display("Request error: {reason}"))]
    RequestError { reason: String },

    #[snafu(display("Key not found: {key_type} at {path} - {reason}"))]
    KeyNotFound { key_type: String, path: String, reason: String },

    #[snafu(display("IO error: {reason}"))]
    IOError { reason: String },
}

// Type aliases for convenience
pub type Error = ZkpAuthError;
pub type Result<T> = std::result::Result<T, ZkpAuthError>;

impl From<ark_serialize::SerializationError> for ZkpAuthError {
    fn from(err: ark_serialize::SerializationError) -> Self {
        ZkpAuthError::SerializationError {
            reason: err.to_string(),
        }
    }
}

impl From<reqwest::Error> for ZkpAuthError {
    fn from(err: reqwest::Error) -> Self {
        ZkpAuthError::RequestError {
            reason: err.to_string(),
        }
    }
}