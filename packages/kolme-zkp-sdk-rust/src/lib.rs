//! # Kolme ZKP SDK for Rust
//!
//! A comprehensive Rust SDK for integrating with the Kolme Zero-Knowledge Proof (ZKP)
//! social authentication system. This SDK provides a complete client library for
//! OAuth-based social authentication with cryptographic proof generation and verification.
//!
//! ## Features
//!
//! - **OAuth Integration**: Support for Twitter, GitHub, Discord, and Google
//! - **Zero-Knowledge Proofs**: Generate and verify cryptographic proofs using Groth16
//! - **Session Management**: Secure session handling with automatic cleanup
//! - **Event System**: Comprehensive event notifications for all operations
//! - **Retry Logic**: Built-in retry mechanisms for handling transient failures
//! - **Type Safety**: Full Rust type safety with comprehensive error handling
//! - **Async/Await**: Modern async Rust API with tokio support
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use kolme_zkp_sdk::{KolmeZkpSdk, KolmeZkpConfig, SocialPlatform};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create SDK instance
//!     let sdk = KolmeZkpSdk::with_api_url("http://localhost:8080")?;
//!
//!     // Initialize OAuth flow
//!     let auth_response = sdk.init_auth(
//!         SocialPlatform::GitHub,
//!         "http://localhost:3000/callback",
//!         None, // Optional public key
//!     ).await?;
//!
//!     println!("Visit: {}", auth_response.auth_url);
//!
//!     // After user completes OAuth, handle callback
//!     let callback_response = sdk.handle_callback("auth_code", "state").await?;
//!
//!     if let Some(session_id) = callback_response.session_id {
//!         println!("Authentication successful! Session: {}", session_id);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## OAuth Flow Example
//!
//! ```rust,no_run
//! use kolme_zkp_sdk::{KolmeZkpSdk, SocialPlatform, CryptoUtils};
//!
//! async fn complete_auth_flow() -> Result<(), Box<dyn std::error::Error>> {
//!     let sdk = KolmeZkpSdk::with_api_url("http://localhost:8080")?;
//!
//!     // Generate keypair for signing
//!     let (signing_key, public_key) = CryptoUtils::generate_keypair()?;
//!
//!     // Initialize OAuth with public key
//!     let auth_response = sdk.init_auth(
//!         SocialPlatform::Twitter,
//!         "http://localhost:3000/callback",
//!         Some(public_key),
//!     ).await?;
//!
//!     // User visits auth_response.auth_url and completes OAuth
//!     // Your application receives callback with code and state
//!
//!     let callback_response = sdk.handle_callback("received_code", "received_state").await?;
//!
//!     if let Some(session_id) = callback_response.session_id {
//!         // Create signature for proof generation
//!         let signature = CryptoUtils::create_session_signature(&signing_key, &session_id)?;
//!
//!         // Generate ZKP proof
//!         let proof_response = sdk.prove_identity(session_id, signature).await?;
//!
//!         println!("Proof generated successfully!");
//!         println!("Commitment: {:?}", proof_response.commitment);
//!     }
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Event Handling
//!
//! ```rust,no_run
//! use kolme_zkp_sdk::{KolmeZkpSdk, EventListener, KolmeZkpEvent};
//! use std::sync::Arc;
//!
//! struct MyEventListener;
//!
//! #[async_trait::async_trait]
//! impl EventListener for MyEventListener {
//!     async fn on_event(&self, event: KolmeZkpEvent) {
//!         match event {
//!             KolmeZkpEvent::AuthCompleted { identity, .. } => {
//!                 let username = identity.username.as_deref().unwrap_or("Unknown");
//!                 println!("User authenticated: {}", username);
//!             }
//!             KolmeZkpEvent::ProofGenerated { .. } => {
//!                 println!("ZKP proof generated successfully");
//!             }
//!             KolmeZkpEvent::Error { error, .. } => {
//!                 eprintln!("SDK error: {}", error);
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//!
//! async fn setup_events() -> Result<(), Box<dyn std::error::Error>> {
//!     let sdk = KolmeZkpSdk::with_api_url("http://localhost:8080")?;
//!     sdk.add_event_listener(Arc::new(MyEventListener)).await;
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![allow(clippy::module_inception)]

/// HTTP client implementation for API communication
pub mod client;
/// Error types and handling for the SDK
pub mod error;
/// Main SDK implementation and client interface
pub mod sdk;
/// Type definitions and data structures
pub mod types;
/// Utility functions for cryptography, validation, and helpers
pub mod utils;

// Re-export main types for convenience
pub use client::HttpClient;
pub use error::{KolmeZkpError, Result};
pub use sdk::KolmeZkpSdk;
pub use types::{
    AuthSession, EventListener, KolmeZkpConfig, KolmeZkpEvent, PublicKey, Signature,
    SocialIdentity, SocialIdentityCommitment, SocialPlatform, ZkProof,
    InitAuthRequest, InitAuthResponse, CallbackRequest, CallbackResponse,
    ProveIdentityRequest, ProveIdentityResponse, VerifyProofRequest, VerifyProofResponse,
    StatusQuery, StatusResponse,
};
pub use utils::{CryptoUtils, PlatformUtils, RetryUtils, UrlUtils, ValidationUtils};

/// SDK version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// SDK user agent string
pub const USER_AGENT: &str = concat!("kolme-zkp-sdk-rust/", env!("CARGO_PKG_VERSION"));

/// Default API timeout in seconds
pub const DEFAULT_TIMEOUT: u64 = 30;

/// Default session expiry time in hours
pub const DEFAULT_SESSION_EXPIRY_HOURS: i64 = 24;

/// Maximum retry attempts for transient failures
pub const DEFAULT_MAX_RETRIES: usize = 3;

/// Prelude module for convenient imports
pub mod prelude {
    //! Convenient re-exports for common usage

    pub use crate::{
        KolmeZkpSdk, KolmeZkpConfig, KolmeZkpError, Result,
        SocialPlatform, SocialIdentity, SocialIdentityCommitment, ZkProof,
        PublicKey, Signature, AuthSession, EventListener, KolmeZkpEvent,
        CryptoUtils, PlatformUtils, UrlUtils, ValidationUtils,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_info() {
        assert!(!VERSION.is_empty());
        assert!(USER_AGENT.contains("kolme-zkp-sdk-rust"));
    }

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_TIMEOUT, 30);
        assert_eq!(DEFAULT_SESSION_EXPIRY_HOURS, 24);
        assert_eq!(DEFAULT_MAX_RETRIES, 3);
    }
}
