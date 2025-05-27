use kolme_zkp_sdk::{
    KolmeZkpSdk, KolmeZkpConfig, SocialPlatform, EventListener, KolmeZkpEvent,
    CryptoUtils, UrlUtils, ValidationUtils, PlatformUtils,
    SocialIdentityCommitment, ZkProof, PublicKey, Signature,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Test event listener that captures events for verification
#[derive(Debug, Default)]
struct TestEventListener {
    events: Arc<Mutex<Vec<KolmeZkpEvent>>>,
}

impl TestEventListener {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    async fn get_events(&self) -> Vec<KolmeZkpEvent> {
        self.events.lock().await.clone()
    }

    #[allow(dead_code)]
    async fn clear_events(&self) {
        self.events.lock().await.clear();
    }

    #[allow(dead_code)]
    async fn wait_for_event_count(&self, count: usize, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < timeout_ms as u128 {
            if self.events.lock().await.len() >= count {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        false
    }
}

#[async_trait::async_trait]
impl EventListener for TestEventListener {
    async fn on_event(&self, event: KolmeZkpEvent) {
        self.events.lock().await.push(event);
    }
}

#[tokio::test]
async fn test_sdk_creation() {
    let config = KolmeZkpConfig::new("http://localhost:8080");
    let sdk = KolmeZkpSdk::new(config);
    assert!(sdk.is_ok());
}

#[tokio::test]
async fn test_sdk_with_custom_config() {
    let config = KolmeZkpConfig::new("http://localhost:8080")
        .with_timeout(60)
        .with_header("X-Test", "integration")
        .with_insecure_ssl();

    let sdk = KolmeZkpSdk::new(config);
    assert!(sdk.is_ok());

    let sdk = sdk.unwrap();
    assert_eq!(sdk.config().api_url, "http://localhost:8080");
    assert_eq!(sdk.config().timeout, Some(60));
    assert!(!sdk.config().verify_ssl);
}

#[tokio::test]
async fn test_event_listener() {
    let config = KolmeZkpConfig::new("http://localhost:8080");
    let sdk = KolmeZkpSdk::new(config).unwrap();

    let listener = Arc::new(TestEventListener::new());
    sdk.add_event_listener(listener.clone()).await;

    // Events should be empty initially
    let events = listener.get_events().await;
    assert_eq!(events.len(), 0);

    // Clear listeners
    sdk.clear_event_listeners().await;
}

#[tokio::test]
async fn test_crypto_utils() {
    // Test keypair generation
    let (signing_key, public_key) = CryptoUtils::generate_keypair().unwrap();
    assert_eq!(public_key.as_bytes().len(), 32);

    // Test signing and verification
    let message = b"test message for signing";
    let signature = CryptoUtils::sign_message(&signing_key, message).unwrap();

    let is_valid = CryptoUtils::verify_signature(&public_key, message, &signature).unwrap();
    assert!(is_valid);

    // Test with wrong message
    let wrong_message = b"wrong message";
    let is_valid = CryptoUtils::verify_signature(&public_key, wrong_message, &signature).unwrap();
    assert!(!is_valid);

    // Test session signature
    let session_id = "test-session-123";
    let session_signature = CryptoUtils::create_session_signature(&signing_key, session_id).unwrap();

    let is_valid = CryptoUtils::verify_session_signature(&public_key, session_id, &session_signature).unwrap();
    assert!(is_valid);

    // Test with wrong session ID
    let wrong_session_id = "wrong-session-456";
    let is_valid = CryptoUtils::verify_session_signature(&public_key, wrong_session_id, &session_signature).unwrap();
    assert!(!is_valid);
}

#[tokio::test]
async fn test_url_utils() {
    // Test OAuth callback parsing
    let callback_url = "https://example.com/callback?code=abc123&state=xyz789&extra=ignored";
    let (code, state) = UrlUtils::parse_oauth_callback(callback_url).unwrap();
    assert_eq!(code, "abc123");
    assert_eq!(state, "xyz789");

    // Test missing parameters
    let invalid_url = "https://example.com/callback?code=abc123";
    let result = UrlUtils::parse_oauth_callback(invalid_url);
    assert!(result.is_err());

    // Test redirect URI building
    let redirect_uri = UrlUtils::build_redirect_uri("https://example.com", "/auth/callback").unwrap();
    assert_eq!(redirect_uri, "https://example.com/auth/callback");

    let redirect_uri = UrlUtils::build_redirect_uri("https://example.com/", "auth/callback").unwrap();
    assert_eq!(redirect_uri, "https://example.com/auth/callback");

    // Test redirect URI validation
    assert!(UrlUtils::validate_redirect_uri("https://example.com/callback").is_ok());
    assert!(UrlUtils::validate_redirect_uri("http://localhost:3000/callback").is_ok());
    assert!(UrlUtils::validate_redirect_uri("ftp://example.com/callback").is_err());
    assert!(UrlUtils::validate_redirect_uri("invalid-url").is_err());
}

#[tokio::test]
async fn test_platform_utils() {
    // Test display names
    assert_eq!(PlatformUtils::platform_display_name(SocialPlatform::Twitter), "Twitter");
    assert_eq!(PlatformUtils::platform_display_name(SocialPlatform::GitHub), "GitHub");
    assert_eq!(PlatformUtils::platform_display_name(SocialPlatform::Discord), "Discord");
    assert_eq!(PlatformUtils::platform_display_name(SocialPlatform::Google), "Google");

    // Test OAuth scopes
    assert!(!PlatformUtils::platform_oauth_scope(SocialPlatform::Twitter).is_empty());
    assert!(!PlatformUtils::platform_oauth_scope(SocialPlatform::GitHub).is_empty());
    assert!(!PlatformUtils::platform_oauth_scope(SocialPlatform::Discord).is_empty());
    assert!(!PlatformUtils::platform_oauth_scope(SocialPlatform::Google).is_empty());

    // Test email support
    assert!(!PlatformUtils::platform_supports_email(SocialPlatform::Twitter));
    assert!(PlatformUtils::platform_supports_email(SocialPlatform::GitHub));
    assert!(PlatformUtils::platform_supports_email(SocialPlatform::Discord));
    assert!(PlatformUtils::platform_supports_email(SocialPlatform::Google));

    // Test user ID formats
    assert!(!PlatformUtils::platform_user_id_format(SocialPlatform::Twitter).is_empty());
    assert!(!PlatformUtils::platform_user_id_format(SocialPlatform::GitHub).is_empty());
    assert!(!PlatformUtils::platform_user_id_format(SocialPlatform::Discord).is_empty());
    assert!(!PlatformUtils::platform_user_id_format(SocialPlatform::Google).is_empty());
}

#[tokio::test]
async fn test_validation_utils() {
    // Test session ID validation
    assert!(ValidationUtils::validate_session_id("valid-session-id-123").is_ok());
    assert!(ValidationUtils::validate_session_id("another_valid_session_456").is_ok());

    assert!(ValidationUtils::validate_session_id("").is_err());
    assert!(ValidationUtils::validate_session_id("short").is_err());
    assert!(ValidationUtils::validate_session_id("invalid@session#id").is_err());
    assert!(ValidationUtils::validate_session_id(&"x".repeat(200)).is_err());

    // Test user ID validation
    assert!(ValidationUtils::validate_user_id(SocialPlatform::Twitter, "123456789").is_ok());
    assert!(ValidationUtils::validate_user_id(SocialPlatform::GitHub, "987654321").is_ok());
    assert!(ValidationUtils::validate_user_id(SocialPlatform::Discord, "123456789012345678").is_ok());
    assert!(ValidationUtils::validate_user_id(SocialPlatform::Google, "123456789012345678901").is_ok());

    assert!(ValidationUtils::validate_user_id(SocialPlatform::Twitter, "").is_err());
    assert!(ValidationUtils::validate_user_id(SocialPlatform::Twitter, "invalid_id").is_err());
    assert!(ValidationUtils::validate_user_id(SocialPlatform::Discord, "123").is_err());

    // Test commitment validation
    let valid_commitment = [1u8; 32];
    assert!(ValidationUtils::validate_commitment(&valid_commitment).is_ok());

    let invalid_commitment = [0u8; 32];
    assert!(ValidationUtils::validate_commitment(&invalid_commitment).is_err());
}

#[tokio::test]
async fn test_public_key_serialization() {
    let (_, public_key) = CryptoUtils::generate_keypair().unwrap();

    // Test hex conversion
    let hex_string = public_key.to_hex();
    assert_eq!(hex_string.len(), 64); // 32 bytes * 2 hex chars

    let parsed_key = PublicKey::from_hex(&hex_string).unwrap();
    assert_eq!(public_key.as_bytes(), parsed_key.as_bytes());

    // Test invalid hex
    assert!(PublicKey::from_hex("invalid_hex").is_err());
    assert!(PublicKey::from_hex("").is_err());
    assert!(PublicKey::from_hex(&"a".repeat(63)).is_err()); // Wrong length
}

#[tokio::test]
async fn test_signature_serialization() {
    let (secret_key, _) = CryptoUtils::generate_keypair().unwrap();
    let message = b"test message";
    let signature = CryptoUtils::sign_message(&secret_key, message).unwrap();

    // Test hex conversion
    let hex_string = signature.to_hex();
    assert_eq!(hex_string.len(), 128); // 64 bytes * 2 hex chars

    let parsed_signature = Signature::from_hex(&hex_string).unwrap();
    assert_eq!(signature.as_bytes(), parsed_signature.as_bytes());

    // Test invalid hex
    assert!(Signature::from_hex("invalid_hex").is_err());
    assert!(Signature::from_hex("").is_err());
    assert!(Signature::from_hex(&"a".repeat(127)).is_err()); // Wrong length
}

#[tokio::test]
async fn test_session_management() {
    let config = KolmeZkpConfig::new("http://localhost:8080");
    let sdk = KolmeZkpSdk::new(config).unwrap();

    // Initially no sessions
    let sessions = sdk.list_local_sessions().await;
    assert_eq!(sessions.len(), 0);

    // Session should not exist
    let session = sdk.get_local_session("nonexistent").await;
    assert!(session.is_none());

    // Remove non-existent session
    let removed = sdk.remove_local_session("nonexistent").await;
    assert!(!removed);

    // Cleanup should remove 0 sessions
    let cleaned = sdk.cleanup_expired_sessions().await;
    assert_eq!(cleaned, 0);
}

#[tokio::test]
async fn test_error_categorization() {
    use kolme_zkp_sdk::KolmeZkpError;

    let crypto_error = KolmeZkpError::crypto("test crypto error");
    assert_eq!(crypto_error.category(), "crypto");
    assert!(crypto_error.is_auth_error());
    assert!(!crypto_error.is_retryable());

    let session_error = KolmeZkpError::session("session expired");
    assert_eq!(session_error.category(), "session");
    assert!(session_error.is_session_expired());
    assert!(session_error.is_auth_error());

    let timeout_error = KolmeZkpError::Timeout;
    assert_eq!(timeout_error.category(), "timeout");
    assert!(timeout_error.is_retryable());
    assert!(!timeout_error.is_auth_error());

    let config_error = KolmeZkpError::config("invalid config");
    assert_eq!(config_error.category(), "config");
    assert!(!config_error.is_retryable());
    assert!(!config_error.is_session_expired());
}

#[tokio::test]
async fn test_type_safety() {
    // Test that types are properly constrained and safe to use
    let platform = SocialPlatform::GitHub;
    let commitment = SocialIdentityCommitment {
        commitment: [1u8; 32],
        platform,
    };

    let proof = ZkProof {
        proof_data: vec![0u8; 192],
        public_inputs: vec![[0u8; 32]],
        platform,
    };

    // These should compile and be safe
    assert_eq!(commitment.platform, platform);
    assert_eq!(proof.platform, platform);
    assert_eq!(proof.proof_data.len(), 192);
}

#[tokio::test]
async fn test_concurrent_operations() {
    let config = KolmeZkpConfig::new("http://localhost:8080");
    let sdk = Arc::new(KolmeZkpSdk::new(config).unwrap());

    // Test concurrent session operations
    let mut handles = Vec::new();

    for i in 0..10 {
        let sdk_clone = sdk.clone();
        let handle = tokio::spawn(async move {
            let session_id = format!("concurrent-session-{}", i);

            // These operations should be thread-safe
            let _ = sdk_clone.get_local_session(&session_id).await;
            let _ = sdk_clone.list_local_sessions().await;
            let _ = sdk_clone.cleanup_expired_sessions().await;

            i
        });
        handles.push(handle);
    }

    // Wait for all operations to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result < 10);
    }
}
