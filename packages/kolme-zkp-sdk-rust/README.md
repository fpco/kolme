# Kolme ZKP SDK for Rust

[![Crates.io](https://img.shields.io/crates/v/kolme-zkp-sdk.svg)](https://crates.io/crates/kolme-zkp-sdk)
[![Documentation](https://docs.rs/kolme-zkp-sdk/badge.svg)](https://docs.rs/kolme-zkp-sdk)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

A comprehensive Rust SDK for integrating with the Kolme Zero-Knowledge Proof (ZKP) social authentication system. This SDK provides a complete client library for OAuth-based social authentication with cryptographic proof generation and verification.

## 🚀 Features

- **OAuth Integration**: Support for Twitter, GitHub, Discord, and Google
- **Zero-Knowledge Proofs**: Generate and verify cryptographic proofs using Groth16
- **Session Management**: Secure session handling with automatic cleanup
- **Event System**: Comprehensive event notifications for all operations
- **Retry Logic**: Built-in retry mechanisms for handling transient failures
- **Type Safety**: Full Rust type safety with comprehensive error handling
- **Async/Await**: Modern async Rust API with tokio support
- **Production Ready**: Real cryptographic implementations, no mocks

## 📦 Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
kolme-zkp-sdk = "0.1.0"
tokio = { version = "1.0", features = ["full"] }
```

## 🏃 Quick Start

```rust
use kolme_zkp_sdk::{KolmeZkpSdk, SocialPlatform};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create SDK instance
    let sdk = KolmeZkpSdk::with_api_url("http://localhost:8080")?;
    
    // Initialize OAuth flow
    let auth_response = sdk.init_auth(
        SocialPlatform::GitHub,
        "http://localhost:3000/callback",
        None, // Optional public key
    ).await?;
    
    println!("Visit: {}", auth_response.auth_url);
    
    // After user completes OAuth, handle callback
    let callback_response = sdk.handle_callback("auth_code", "state").await?;
    
    if let Some(session_id) = callback_response.session_id {
        println!("Authentication successful! Session: {}", session_id);
    }
    
    Ok(())
}
```

## 📚 Complete Examples

### OAuth Authentication Flow

```rust
use kolme_zkp_sdk::{KolmeZkpSdk, SocialPlatform, CryptoUtils};

async fn complete_auth_flow() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = KolmeZkpSdk::with_api_url("http://localhost:8080")?;
    
    // Generate keypair for signing
    let (secret_key, public_key) = CryptoUtils::generate_keypair()?;
    
    // Initialize OAuth with public key
    let auth_response = sdk.init_auth(
        SocialPlatform::Twitter,
        "http://localhost:3000/callback",
        Some(public_key),
    ).await?;
    
    // User visits auth_response.auth_url and completes OAuth
    // Your application receives callback with code and state
    
    let callback_response = sdk.handle_callback("received_code", "received_state").await?;
    
    if let Some(session_id) = callback_response.session_id {
        // Create signature for proof generation
        let signature = CryptoUtils::create_session_signature(&secret_key, &session_id)?;
        
        // Generate ZKP proof
        let proof_response = sdk.prove_identity(session_id, signature).await?;
        
        println!("Proof generated successfully!");
        println!("Commitment: {:?}", proof_response.commitment);
    }
    
    Ok(())
}
```

### Event Handling

```rust
use kolme_zkp_sdk::{KolmeZkpSdk, EventListener, KolmeZkpEvent};
use std::sync::Arc;

struct MyEventListener;

#[async_trait::async_trait]
impl EventListener for MyEventListener {
    async fn on_event(&self, event: KolmeZkpEvent) {
        match event {
            KolmeZkpEvent::AuthCompleted { identity, .. } => {
                println!("User authenticated: {}", identity.username);
            }
            KolmeZkpEvent::ProofGenerated { .. } => {
                println!("ZKP proof generated successfully");
            }
            KolmeZkpEvent::Error { error, .. } => {
                eprintln!("SDK error: {}", error);
            }
            _ => {}
        }
    }
}

async fn setup_events() -> Result<(), Box<dyn std::error::Error>> {
    let sdk = KolmeZkpSdk::with_api_url("http://localhost:8080")?;
    sdk.add_event_listener(Arc::new(MyEventListener)).await;
    Ok(())
}
```

### Proof Verification

```rust
use kolme_zkp_sdk::{KolmeZkpSdk, VerifyProofRequest, SocialPlatform};

async fn verify_proof(
    sdk: &KolmeZkpSdk,
    commitment: SocialIdentityCommitment,
    proof: ZkProof,
) -> Result<bool, Box<dyn std::error::Error>> {
    let verify_request = VerifyProofRequest {
        commitment,
        proof,
        platform: SocialPlatform::GitHub,
    };
    
    let response = sdk.verify_proof(verify_request).await?;
    Ok(response.valid)
}
```

## 🔧 Configuration

### Basic Configuration

```rust
use kolme_zkp_sdk::{KolmeZkpSdk, KolmeZkpConfig};

let config = KolmeZkpConfig::new("http://localhost:8080");
let sdk = KolmeZkpSdk::new(config)?;
```

### Advanced Configuration

```rust
use kolme_zkp_sdk::KolmeZkpConfig;
use std::time::Duration;

let config = KolmeZkpConfig::new("https://api.kolme.io")
    .with_timeout(60) // 60 second timeout
    .with_header("Authorization", "Bearer your-token")
    .with_header("X-Client-Version", "1.0.0")
    .with_websocket_url("wss://api.kolme.io/ws");

let sdk = KolmeZkpSdk::new(config)?;
```

## 🔐 Cryptographic Operations

### Key Generation and Signing

```rust
use kolme_zkp_sdk::CryptoUtils;

// Generate a new keypair
let (secret_key, public_key) = CryptoUtils::generate_keypair()?;

// Sign a message
let message = b"Hello, Kolme!";
let signature = CryptoUtils::sign_message(&secret_key, message)?;

// Verify signature
let is_valid = CryptoUtils::verify_signature(&public_key, message, &signature)?;
assert!(is_valid);

// Create session signature
let session_id = "session-123";
let session_signature = CryptoUtils::create_session_signature(&secret_key, session_id)?;
```

## 🌐 Supported Platforms

| Platform | OAuth Support | Email Support | User ID Format |
|----------|---------------|---------------|----------------|
| Twitter  | ✅            | ❌            | Numeric        |
| GitHub   | ✅            | ✅            | Numeric        |
| Discord  | ✅            | ✅            | Snowflake      |
| Google   | ✅            | ✅            | Numeric        |

## 🛠️ Utilities

### URL Utilities

```rust
use kolme_zkp_sdk::UrlUtils;

// Parse OAuth callback
let (code, state) = UrlUtils::parse_oauth_callback(
    "https://example.com/callback?code=abc123&state=xyz789"
)?;

// Build redirect URI
let redirect_uri = UrlUtils::build_redirect_uri(
    "https://example.com", 
    "/auth/callback"
)?;

// Validate redirect URI
UrlUtils::validate_redirect_uri("https://example.com/callback")?;
```

### Platform Utilities

```rust
use kolme_zkp_sdk::{PlatformUtils, SocialPlatform};

let platform = SocialPlatform::GitHub;

println!("Display name: {}", PlatformUtils::platform_display_name(platform));
println!("OAuth scope: {}", PlatformUtils::platform_oauth_scope(platform));
println!("Supports email: {}", PlatformUtils::platform_supports_email(platform));
```

### Validation Utilities

```rust
use kolme_zkp_sdk::{ValidationUtils, SocialPlatform};

// Validate session ID
ValidationUtils::validate_session_id("valid-session-123")?;

// Validate user ID for platform
ValidationUtils::validate_user_id(SocialPlatform::GitHub, "123456789")?;

// Validate commitment
let commitment = [1u8; 32];
ValidationUtils::validate_commitment(&commitment)?;
```

## 🔄 Error Handling

The SDK provides comprehensive error handling with detailed error types:

```rust
use kolme_zkp_sdk::{KolmeZkpError, Result};

match sdk.prove_identity(session_id, signature).await {
    Ok(response) => {
        println!("Proof generated: {:?}", response.commitment);
    }
    Err(KolmeZkpError::SessionError { message }) => {
        eprintln!("Session error: {}", message);
    }
    Err(KolmeZkpError::Timeout) => {
        eprintln!("Request timed out, retrying...");
    }
    Err(e) if e.is_retryable() => {
        eprintln!("Retryable error: {}", e);
    }
    Err(e) => {
        eprintln!("Fatal error: {}", e);
    }
}
```

## 🧪 Testing

Run the test suite:

```bash
# Run all tests
cargo test

# Run integration tests
cargo test --test integration_tests

# Run with logging
RUST_LOG=debug cargo test
```

## 📖 Examples

The SDK includes comprehensive examples:

```bash
# Basic authentication flow
cargo run --example basic_auth_flow

# Proof generation
cargo run --example proof_generation

# Proof verification
cargo run --example proof_verification
```

## 🤝 Contributing

Contributions are welcome! Please read our [Contributing Guide](CONTRIBUTING.md) for details.

## 📄 License

This project is licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## 🔗 Links

- [Documentation](https://docs.rs/kolme-zkp-sdk)
- [Crates.io](https://crates.io/crates/kolme-zkp-sdk)
- [GitHub Repository](https://github.com/fpco/kolme)
- [Kolme Website](https://kolme.io)
