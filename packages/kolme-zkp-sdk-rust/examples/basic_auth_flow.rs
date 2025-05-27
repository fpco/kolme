//! Basic OAuth authentication flow example
//!
//! This example demonstrates how to use the Kolme ZKP SDK to perform
//! a complete OAuth authentication flow with a social platform.
//!
//! Run with: cargo run --example basic_auth_flow

use kolme_zkp_sdk::{
    KolmeZkpSdk, KolmeZkpConfig, SocialPlatform, EventListener, KolmeZkpEvent,
    CryptoUtils, UrlUtils, Result,
};
use std::sync::Arc;
use std::io::{self, Write};

/// Simple event listener that logs all events
struct ConsoleEventListener;

#[async_trait::async_trait]
impl EventListener for ConsoleEventListener {
    async fn on_event(&self, event: KolmeZkpEvent) {
        match event {
            KolmeZkpEvent::AuthStarted { platform } => {
                println!("🚀 Authentication started for platform: {:?}", platform);
            }
            KolmeZkpEvent::AuthCompleted { identity, session_id } => {
                println!("✅ Authentication completed!");
                println!("   User: {} ({})", identity.username.as_deref().unwrap_or("N/A"), identity.user_id);
                println!("   Platform: {:?}", identity.platform);
                if let Some(session_id) = session_id {
                    println!("   Session ID: {}", session_id);
                }
            }
            KolmeZkpEvent::AuthFailed { error } => {
                println!("❌ Authentication failed: {}", error);
            }
            KolmeZkpEvent::ProofGenerated { commitment, .. } => {
                println!("🔐 ZKP proof generated successfully!");
                println!("   Commitment: {}", hex::encode(commitment.commitment));
            }
            KolmeZkpEvent::ProofVerified { valid, platform } => {
                if valid {
                    println!("✅ Proof verified successfully for platform: {:?}", platform);
                } else {
                    println!("❌ Proof verification failed");
                }
            }
            KolmeZkpEvent::SessionExpired { session_id } => {
                println!("⏰ Session expired: {}", session_id);
            }
            KolmeZkpEvent::Error { error, details } => {
                println!("💥 Error: {}", error);
                if let Some(details) = details {
                    println!("   Details: {}", details);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    env_logger::init();

    println!("🎯 Kolme ZKP SDK - Basic Authentication Flow Example");
    println!("==================================================");

    // Create SDK configuration
    let config = KolmeZkpConfig::new("http://localhost:8080")
        .with_timeout(60) // 60 second timeout
        .with_header("X-Client", "kolme-zkp-sdk-rust-example");

    // Create SDK instance
    let sdk = KolmeZkpSdk::new(config)?;

    // Add event listener
    sdk.add_event_listener(Arc::new(ConsoleEventListener)).await;

    // Generate a keypair for signing
    println!("\n🔑 Generating cryptographic keypair...");
    let (signing_key, public_key) = CryptoUtils::generate_keypair()?;
    println!("   Public key: {}", public_key.to_hex());

    // Choose platform
    let platform = choose_platform()?;
    println!("\n📱 Selected platform: {:?}", platform);

    // Get redirect URI
    let redirect_uri = get_redirect_uri()?;
    println!("   Redirect URI: {}", redirect_uri);

    // Initialize OAuth flow
    println!("\n🔄 Initializing OAuth flow...");
    let auth_response = sdk.init_auth(platform, &redirect_uri, Some(public_key)).await?;

    println!("\n🌐 Please visit the following URL to authenticate:");
    println!("   {}", auth_response.auth_url);
    println!("\n📋 OAuth state: {}", auth_response.state);

    // Wait for user to complete OAuth and provide callback URL
    let callback_url = get_callback_url()?;

    // Parse callback URL
    let (code, state) = UrlUtils::parse_oauth_callback(&callback_url)?;

    // Verify state matches
    if state != auth_response.state {
        return Err(kolme_zkp_sdk::KolmeZkpError::auth("OAuth state mismatch"));
    }

    // Handle OAuth callback
    println!("\n🔄 Processing OAuth callback...");
    let callback_response = sdk.handle_callback(code, state).await?;

    if !callback_response.success {
        return Err(kolme_zkp_sdk::KolmeZkpError::auth("OAuth callback failed"));
    }

    let session_id = callback_response.session_id
        .ok_or_else(|| kolme_zkp_sdk::KolmeZkpError::auth("No session ID returned"))?;

    println!("\n✅ OAuth authentication successful!");
    println!("   Session ID: {}", session_id);

    // Generate ZKP proof
    if should_generate_proof()? {
        println!("\n🔐 Generating ZKP proof...");
        let signature = CryptoUtils::create_session_signature(&signing_key, &session_id)?;
        let proof_response = sdk.prove_identity(&session_id, signature).await?;

        println!("✅ ZKP proof generated successfully!");
        println!("   Commitment: {}", hex::encode(proof_response.commitment.commitment));
        println!("   Proof size: {} bytes", proof_response.proof.proof_data.len());

        // Verify the proof
        if should_verify_proof()? {
            println!("\n🔍 Verifying ZKP proof...");
            let verify_request = kolme_zkp_sdk::VerifyProofRequest {
                commitment: proof_response.commitment,
                proof: proof_response.proof,
                platform,
            };

            let verify_response = sdk.verify_proof(verify_request).await?;

            if verify_response.valid {
                println!("✅ Proof verification successful!");
            } else {
                println!("❌ Proof verification failed!");
            }
        }
    }

    // Check session status
    println!("\n📊 Checking session status...");
    let status = sdk.get_session_status(&session_id).await?;

    if status.authenticated {
        println!("✅ Session is still active");
        if let Some(identity) = status.identity {
            println!("   User: {} ({})", identity.username.as_deref().unwrap_or("N/A"), identity.user_id);
        }
    } else {
        println!("❌ Session is no longer active");
    }

    // List local sessions
    let local_sessions = sdk.list_local_sessions().await;
    println!("\n📋 Local sessions: {}", local_sessions.len());
    for session in local_sessions {
        println!("   - {} ({})", session.session_id, session.identity.username.as_deref().unwrap_or("N/A"));
    }

    println!("\n🎉 Example completed successfully!");
    Ok(())
}

fn choose_platform() -> Result<SocialPlatform> {
    println!("\nChoose a social platform:");
    println!("1. Twitter");
    println!("2. GitHub");
    println!("3. Discord");
    println!("4. Google");

    print!("Enter choice (1-4): ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    match input.trim() {
        "1" => Ok(SocialPlatform::Twitter),
        "2" => Ok(SocialPlatform::GitHub),
        "3" => Ok(SocialPlatform::Discord),
        "4" => Ok(SocialPlatform::Google),
        _ => {
            println!("Invalid choice, defaulting to GitHub");
            Ok(SocialPlatform::GitHub)
        }
    }
}

fn get_redirect_uri() -> Result<String> {
    print!("Enter redirect URI (default: http://localhost:3000/callback): ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let uri = input.trim();
    if uri.is_empty() {
        Ok("http://localhost:3000/callback".to_string())
    } else {
        UrlUtils::validate_redirect_uri(uri)?;
        Ok(uri.to_string())
    }
}

fn get_callback_url() -> Result<String> {
    println!("\nAfter completing OAuth, paste the full callback URL here:");
    print!("Callback URL: ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    Ok(input.trim().to_string())
}

fn should_generate_proof() -> Result<bool> {
    print!("\nGenerate ZKP proof? (y/N): ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    Ok(input.trim().to_lowercase() == "y")
}

fn should_verify_proof() -> Result<bool> {
    print!("Verify the generated proof? (y/N): ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    Ok(input.trim().to_lowercase() == "y")
}
