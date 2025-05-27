use thiserror::Error;
use crate::types::ApiError;

/// Result type for SDK operations
pub type Result<T> = std::result::Result<T, KolmeZkpError>;

/// Comprehensive error types for the Kolme ZKP SDK
#[derive(Error, Debug)]
pub enum KolmeZkpError {
    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// API returned an error response
    #[error("API error: {0}")]
    ApiError(#[from] ApiError),

    /// JSON serialization/deserialization failed
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// URL parsing failed
    #[error("URL parsing error: {0}")]
    UrlError(#[from] url::ParseError),

    /// Hex encoding/decoding failed
    #[error("Hex encoding error: {0}")]
    HexError(#[from] hex::FromHexError),

    /// Cryptographic operation failed
    #[error("Cryptographic error: {message}")]
    CryptoError {
        /// Error message describing the cryptographic failure
        message: String
    },

    /// Session management error
    #[error("Session error: {message}")]
    SessionError {
        /// Error message describing the session issue
        message: String
    },

    /// Authentication flow error
    #[error("Authentication error: {message}")]
    AuthError {
        /// Error message describing the authentication failure
        message: String
    },

    /// Proof generation error
    #[error("Proof generation error: {message}")]
    ProofError {
        /// Error message describing the proof generation failure
        message: String
    },

    /// Proof verification error
    #[error("Proof verification error: {message}")]
    VerificationError {
        /// Error message describing the verification failure
        message: String
    },

    /// Configuration error
    #[error("Configuration error: {message}")]
    ConfigError {
        /// Error message describing the configuration issue
        message: String
    },

    /// Network timeout
    #[error("Request timed out")]
    Timeout,

    /// Invalid state or parameter
    #[error("Invalid state: {message}")]
    InvalidState {
        /// Error message describing the invalid state
        message: String
    },

    /// Feature not supported
    #[error("Feature not supported: {feature}")]
    NotSupported {
        /// Name of the unsupported feature
        feature: String
    },

    /// Generic error with custom message
    #[error("Error: {message}")]
    Custom {
        /// Custom error message
        message: String
    },
}

impl KolmeZkpError {
    /// Create a new cryptographic error
    pub fn crypto(message: impl Into<String>) -> Self {
        Self::CryptoError {
            message: message.into(),
        }
    }

    /// Create a new session error
    pub fn session(message: impl Into<String>) -> Self {
        Self::SessionError {
            message: message.into(),
        }
    }

    /// Create a new authentication error
    pub fn auth(message: impl Into<String>) -> Self {
        Self::AuthError {
            message: message.into(),
        }
    }

    /// Create a new proof error
    pub fn proof(message: impl Into<String>) -> Self {
        Self::ProofError {
            message: message.into(),
        }
    }

    /// Create a new verification error
    pub fn verification(message: impl Into<String>) -> Self {
        Self::VerificationError {
            message: message.into(),
        }
    }

    /// Create a new configuration error
    pub fn config(message: impl Into<String>) -> Self {
        Self::ConfigError {
            message: message.into(),
        }
    }

    /// Create a new invalid state error
    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState {
            message: message.into(),
        }
    }

    /// Create a new not supported error
    pub fn not_supported(feature: impl Into<String>) -> Self {
        Self::NotSupported {
            feature: feature.into(),
        }
    }

    /// Create a new custom error
    pub fn custom(message: impl Into<String>) -> Self {
        Self::Custom {
            message: message.into(),
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::HttpError(e) => {
                // Network errors are generally retryable
                e.is_timeout() || e.is_connect() || e.is_request()
            }
            Self::Timeout => true,
            Self::ApiError(api_error) => {
                // Some API errors might be retryable (e.g., rate limiting)
                api_error.error.contains("rate limit") ||
                api_error.error.contains("temporary") ||
                api_error.error.contains("try again")
            }
            _ => false,
        }
    }

    /// Check if this error indicates an expired session
    pub fn is_session_expired(&self) -> bool {
        match self {
            Self::SessionError { message } => {
                message.contains("expired") || message.contains("invalid")
            }
            Self::AuthError { message } => {
                message.contains("expired") || message.contains("unauthorized")
            }
            Self::ApiError(api_error) => {
                api_error.error.contains("session") &&
                (api_error.error.contains("expired") || api_error.error.contains("invalid"))
            }
            _ => false,
        }
    }

    /// Check if this error indicates invalid credentials
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            Self::AuthError { .. } |
            Self::CryptoError { .. } |
            Self::SessionError { .. }
        )
    }

    /// Get the error category for logging/metrics
    pub fn category(&self) -> &'static str {
        match self {
            Self::HttpError(_) => "http",
            Self::ApiError(_) => "api",
            Self::JsonError(_) => "json",
            Self::UrlError(_) => "url",
            Self::HexError(_) => "hex",
            Self::CryptoError { .. } => "crypto",
            Self::SessionError { .. } => "session",
            Self::AuthError { .. } => "auth",
            Self::ProofError { .. } => "proof",
            Self::VerificationError { .. } => "verification",
            Self::ConfigError { .. } => "config",
            Self::Timeout => "timeout",
            Self::InvalidState { .. } => "invalid_state",
            Self::NotSupported { .. } => "not_supported",
            Self::Custom { .. } => "custom",
        }
    }
}

// Note: From<reqwest::Error> is already implemented via #[from] in the enum

impl Clone for KolmeZkpError {
    fn clone(&self) -> Self {
        match self {
            Self::HttpError(e) => Self::custom(format!("HTTP error: {}", e)),
            Self::ApiError(e) => Self::ApiError(e.clone()),
            Self::JsonError(e) => Self::custom(format!("JSON error: {}", e)),
            Self::UrlError(e) => Self::UrlError(e.clone()),
            Self::HexError(e) => Self::HexError(e.clone()),
            Self::CryptoError { message } => Self::CryptoError { message: message.clone() },
            Self::SessionError { message } => Self::SessionError { message: message.clone() },
            Self::AuthError { message } => Self::AuthError { message: message.clone() },
            Self::ProofError { message } => Self::ProofError { message: message.clone() },
            Self::VerificationError { message } => Self::VerificationError { message: message.clone() },
            Self::ConfigError { message } => Self::ConfigError { message: message.clone() },
            Self::Timeout => Self::Timeout,
            Self::InvalidState { message } => Self::InvalidState { message: message.clone() },
            Self::NotSupported { feature } => Self::NotSupported { feature: feature.clone() },
            Self::Custom { message } => Self::Custom { message: message.clone() },
        }
    }
}

/// Helper trait for converting Results to KolmeZkpError
pub trait IntoKolmeError<T> {
    /// Convert any error type to KolmeZkpError using Display
    fn into_kolme_error(self) -> Result<T>;
}

impl<T, E> IntoKolmeError<T> for std::result::Result<T, E>
where
    E: std::fmt::Display,
{
    fn into_kolme_error(self) -> Result<T> {
        self.map_err(|e| KolmeZkpError::custom(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_categories() {
        assert_eq!(KolmeZkpError::crypto("test").category(), "crypto");
        assert_eq!(KolmeZkpError::session("test").category(), "session");
        assert_eq!(KolmeZkpError::auth("test").category(), "auth");
        assert_eq!(KolmeZkpError::Timeout.category(), "timeout");
    }

    #[test]
    fn test_retryable_errors() {
        assert!(KolmeZkpError::Timeout.is_retryable());
        assert!(!KolmeZkpError::crypto("test").is_retryable());

        let api_error = ApiError {
            error: "rate limit exceeded".to_string(),
            details: None,
        };
        assert!(KolmeZkpError::ApiError(api_error).is_retryable());
    }

    #[test]
    fn test_session_expired() {
        assert!(KolmeZkpError::session("session expired").is_session_expired());
        assert!(KolmeZkpError::auth("unauthorized access").is_session_expired());
        assert!(!KolmeZkpError::crypto("invalid key").is_session_expired());
    }

    #[test]
    fn test_auth_errors() {
        assert!(KolmeZkpError::auth("invalid credentials").is_auth_error());
        assert!(KolmeZkpError::crypto("signature verification failed").is_auth_error());
        assert!(!KolmeZkpError::Timeout.is_auth_error());
    }
}
