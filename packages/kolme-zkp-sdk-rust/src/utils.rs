use std::time::Duration;
use url::Url;
use ed25519_dalek::{SigningKey, VerifyingKey, Signature as Ed25519Signature, Signer, Verifier};
use rand::rngs::OsRng;

use crate::{
    error::{KolmeZkpError, Result},
    types::{PublicKey, Signature, SocialPlatform},
};

/// Cryptographic utilities for the SDK
pub struct CryptoUtils;

impl CryptoUtils {
    /// Generate a new Ed25519 keypair
    pub fn generate_keypair() -> Result<(SigningKey, PublicKey)> {
        let mut csprng = OsRng {};
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let public_key = PublicKey::from_bytes(verifying_key.to_bytes());

        Ok((signing_key, public_key))
    }

    /// Sign a message with a secret key
    pub fn sign_message(signing_key: &SigningKey, message: &[u8]) -> Result<Signature> {
        let signature = signing_key.sign(message);
        Ok(Signature::from_bytes(signature.to_bytes()))
    }

    /// Verify a signature with a public key
    pub fn verify_signature(public_key: &PublicKey, message: &[u8], signature: &Signature) -> Result<bool> {
        let verifying_key = VerifyingKey::from_bytes(public_key.as_bytes())
            .map_err(|e| KolmeZkpError::crypto(format!("Invalid public key: {}", e)))?;

        let ed25519_signature = Ed25519Signature::try_from(signature.as_bytes().as_slice())
            .map_err(|e| KolmeZkpError::crypto(format!("Invalid signature: {}", e)))?;

        match verifying_key.verify(message, &ed25519_signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Create a session signature for authentication
    pub fn create_session_signature(signing_key: &SigningKey, session_id: &str) -> Result<Signature> {
        let message = format!("kolme-zkp-auth:{}", session_id);
        Self::sign_message(signing_key, message.as_bytes())
    }

    /// Verify a session signature
    pub fn verify_session_signature(
        public_key: &PublicKey,
        session_id: &str,
        signature: &Signature,
    ) -> Result<bool> {
        let message = format!("kolme-zkp-auth:{}", session_id);
        Self::verify_signature(public_key, message.as_bytes(), signature)
    }
}

/// URL utilities for OAuth flows
pub struct UrlUtils;

impl UrlUtils {
    /// Parse OAuth callback URL and extract code and state
    pub fn parse_oauth_callback(callback_url: &str) -> Result<(String, String)> {
        let url = Url::parse(callback_url)?;

        let mut code = None;
        let mut state = None;

        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "code" => code = Some(value.into_owned()),
                "state" => state = Some(value.into_owned()),
                _ => {}
            }
        }

        let code = code.ok_or_else(|| KolmeZkpError::auth("Missing 'code' parameter in callback URL"))?;
        let state = state.ok_or_else(|| KolmeZkpError::auth("Missing 'state' parameter in callback URL"))?;

        Ok((code, state))
    }

    /// Build a redirect URI for OAuth flows
    pub fn build_redirect_uri(base_url: &str, path: &str) -> Result<String> {
        let base = Url::parse(base_url)?;
        let redirect_uri = base.join(path)?;
        Ok(redirect_uri.to_string())
    }

    /// Validate a redirect URI
    pub fn validate_redirect_uri(uri: &str) -> Result<()> {
        let url = Url::parse(uri)?;

        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(KolmeZkpError::config("Redirect URI must use HTTP or HTTPS"));
        }

        if url.host().is_none() {
            return Err(KolmeZkpError::config("Redirect URI must have a valid host"));
        }

        Ok(())
    }
}

/// Retry utilities for handling transient failures
pub struct RetryUtils;

impl RetryUtils {
    /// Retry a function with exponential backoff
    pub async fn retry_with_backoff<F, Fut, T>(
        mut operation: F,
        max_attempts: usize,
        initial_delay: Duration,
        max_delay: Duration,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut delay = initial_delay;
        let mut last_error = None;

        for attempt in 1..=max_attempts {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error.clone());

                    if attempt == max_attempts || !error.is_retryable() {
                        break;
                    }

                    log::warn!("Attempt {} failed, retrying in {:?}: {}", attempt, delay, error);
                    tokio::time::sleep(delay).await;

                    // Exponential backoff with jitter
                    delay = std::cmp::min(delay * 2, max_delay);
                    let jitter = Duration::from_millis(rand::random::<u64>() % 1000);
                    delay += jitter;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| KolmeZkpError::custom("All retry attempts failed")))
    }

    /// Simple retry without backoff
    pub async fn retry_simple<F, Fut, T>(
        operation: F,
        max_attempts: usize,
    ) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        Self::retry_with_backoff(
            operation,
            max_attempts,
            Duration::from_millis(100),
            Duration::from_secs(1),
        ).await
    }
}

/// Platform-specific utilities
pub struct PlatformUtils;

impl PlatformUtils {
    /// Get the display name for a social platform
    pub fn platform_display_name(platform: SocialPlatform) -> &'static str {
        match platform {
            SocialPlatform::Twitter => "Twitter",
            SocialPlatform::GitHub => "GitHub",
            SocialPlatform::Discord => "Discord",
            SocialPlatform::Google => "Google",
        }
    }

    /// Get the OAuth scope for a platform
    pub fn platform_oauth_scope(platform: SocialPlatform) -> &'static str {
        match platform {
            SocialPlatform::Twitter => "read:user",
            SocialPlatform::GitHub => "user:email",
            SocialPlatform::Discord => "identify email",
            SocialPlatform::Google => "openid email profile",
        }
    }

    /// Check if a platform supports email verification
    pub fn platform_supports_email(platform: SocialPlatform) -> bool {
        match platform {
            SocialPlatform::Twitter => false,
            SocialPlatform::GitHub => true,
            SocialPlatform::Discord => true,
            SocialPlatform::Google => true,
        }
    }

    /// Get the typical user ID format for a platform
    pub fn platform_user_id_format(platform: SocialPlatform) -> &'static str {
        match platform {
            SocialPlatform::Twitter => "Numeric ID (e.g., 123456789)",
            SocialPlatform::GitHub => "Numeric ID (e.g., 123456)",
            SocialPlatform::Discord => "Snowflake ID (e.g., 123456789012345678)",
            SocialPlatform::Google => "Numeric ID (e.g., 123456789012345678901)",
        }
    }
}

/// Validation utilities
pub struct ValidationUtils;

impl ValidationUtils {
    /// Validate a session ID format
    pub fn validate_session_id(session_id: &str) -> Result<()> {
        if session_id.is_empty() {
            return Err(KolmeZkpError::invalid_state("Session ID cannot be empty"));
        }

        if session_id.len() < 16 || session_id.len() > 128 {
            return Err(KolmeZkpError::invalid_state("Session ID must be between 16 and 128 characters"));
        }

        if !session_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Err(KolmeZkpError::invalid_state("Session ID contains invalid characters"));
        }

        Ok(())
    }

    /// Validate a user ID for a specific platform
    pub fn validate_user_id(platform: SocialPlatform, user_id: &str) -> Result<()> {
        if user_id.is_empty() {
            return Err(KolmeZkpError::invalid_state("User ID cannot be empty"));
        }

        match platform {
            SocialPlatform::Twitter | SocialPlatform::GitHub | SocialPlatform::Google => {
                if !user_id.chars().all(|c| c.is_ascii_digit()) {
                    return Err(KolmeZkpError::invalid_state(
                        format!("{} user ID must be numeric", PlatformUtils::platform_display_name(platform))
                    ));
                }
            }
            SocialPlatform::Discord => {
                if !user_id.chars().all(|c| c.is_ascii_digit()) || user_id.len() < 17 || user_id.len() > 19 {
                    return Err(KolmeZkpError::invalid_state("Discord user ID must be a 17-19 digit snowflake"));
                }
            }
        }

        Ok(())
    }

    /// Validate a commitment format
    pub fn validate_commitment(commitment: &[u8; 32]) -> Result<()> {
        // Check if commitment is all zeros (invalid)
        if commitment.iter().all(|&b| b == 0) {
            return Err(KolmeZkpError::invalid_state("Commitment cannot be all zeros"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_utils_keypair_generation() {
        let (signing_key, public_key) = CryptoUtils::generate_keypair().unwrap();
        assert_eq!(public_key.as_bytes().len(), 32);
        assert_eq!(signing_key.to_bytes().len(), 32);
    }

    #[test]
    fn test_crypto_utils_sign_and_verify() {
        let (signing_key, public_key) = CryptoUtils::generate_keypair().unwrap();
        let message = b"test message";

        let signature = CryptoUtils::sign_message(&signing_key, message).unwrap();
        let is_valid = CryptoUtils::verify_signature(&public_key, message, &signature).unwrap();

        assert!(is_valid);
    }

    #[test]
    fn test_url_utils_parse_callback() {
        let callback_url = "https://example.com/callback?code=abc123&state=xyz789";
        let (code, state) = UrlUtils::parse_oauth_callback(callback_url).unwrap();

        assert_eq!(code, "abc123");
        assert_eq!(state, "xyz789");
    }

    #[test]
    fn test_url_utils_build_redirect_uri() {
        let redirect_uri = UrlUtils::build_redirect_uri("https://example.com", "/auth/callback").unwrap();
        assert_eq!(redirect_uri, "https://example.com/auth/callback");
    }

    #[test]
    fn test_platform_utils() {
        assert_eq!(PlatformUtils::platform_display_name(SocialPlatform::Twitter), "Twitter");
        assert!(PlatformUtils::platform_supports_email(SocialPlatform::GitHub));
        assert!(!PlatformUtils::platform_supports_email(SocialPlatform::Twitter));
    }

    #[test]
    fn test_validation_utils() {
        assert!(ValidationUtils::validate_session_id("valid-session-id-123").is_ok());
        assert!(ValidationUtils::validate_session_id("").is_err());
        assert!(ValidationUtils::validate_session_id("invalid@session").is_err());

        assert!(ValidationUtils::validate_user_id(SocialPlatform::Twitter, "123456789").is_ok());
        assert!(ValidationUtils::validate_user_id(SocialPlatform::Twitter, "invalid").is_err());
    }
}
