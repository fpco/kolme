//! ZKP proof generation example
//!
//! This example demonstrates how to generate zero-knowledge proofs
//! for authenticated social identities using the Kolme ZKP SDK.
//!
//! Run with: cargo run --example proof_generation

use kolme_zkp_sdk::{
    KolmeZkpSdk, KolmeZkpConfig, SocialPlatform, SocialIdentityCommitment,
    CryptoUtils, PlatformUtils, ValidationUtils, Result,
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("🔐 Kolme ZKP SDK - Proof Generation Example");
    println!("==========================================");

    // Create SDK instance
    let config = KolmeZkpConfig::new("http://localhost:8080")
        .with_timeout(120) // Longer timeout for proof generation
        .with_header("X-Example", "proof-generation");

    let sdk = KolmeZkpSdk::new(config)?;

    // Generate keypair
    println!("\n🔑 Generating cryptographic keypair...");
    let (signing_key, public_key) = CryptoUtils::generate_keypair()?;
    println!("   Public key: {}", public_key.to_hex());

    // Simulate an authenticated session (in real usage, this would come from OAuth)
    let session_id = "example-session-12345";
    let platform = SocialPlatform::GitHub;

    println!("\n📱 Simulating authenticated session:");
    println!("   Platform: {:?}", platform);
    println!("   Session ID: {}", session_id);

    // Validate session ID format
    ValidationUtils::validate_session_id(session_id)?;
    println!("✅ Session ID format is valid");

    // Create session signature
    println!("\n✍️  Creating session signature...");
    let signature = CryptoUtils::create_session_signature(&signing_key, session_id)?;
    println!("   Signature: {}", signature.to_hex());

    // Verify signature locally
    let is_valid = CryptoUtils::verify_session_signature(&public_key, session_id, &signature)?;
    if is_valid {
        println!("✅ Signature verification successful");
    } else {
        return Err(kolme_zkp_sdk::KolmeZkpError::crypto("Signature verification failed"));
    }

    // Generate ZKP proof
    println!("\n🔐 Generating ZKP proof...");
    println!("   This may take a few seconds...");

    let start_time = std::time::Instant::now();

    match sdk.prove_identity(session_id, signature).await {
        Ok(proof_response) => {
            let duration = start_time.elapsed();
            println!("✅ ZKP proof generated successfully!");
            println!("   Generation time: {:?}", duration);
            println!("   Commitment: {}", hex::encode(proof_response.commitment.commitment));
            println!("   Platform: {:?}", proof_response.commitment.platform);
            println!("   Proof size: {} bytes", proof_response.proof.proof_data.len());

            // Validate the commitment
            ValidationUtils::validate_commitment(&proof_response.commitment.commitment)?;
            println!("✅ Commitment format is valid");

            // Display proof details
            display_proof_details(&proof_response.commitment, &proof_response.proof);

            // Demonstrate proof verification
            println!("\n🔍 Verifying the generated proof...");
            let verify_request = kolme_zkp_sdk::VerifyProofRequest {
                commitment: proof_response.commitment.clone(),
                proof: proof_response.proof.clone(),
                platform,
            };

            let verify_start = std::time::Instant::now();
            match sdk.verify_proof(verify_request).await {
                Ok(verify_response) => {
                    let verify_duration = verify_start.elapsed();
                    if verify_response.valid {
                        println!("✅ Proof verification successful!");
                        println!("   Verification time: {:?}", verify_duration);
                        println!("   Verified platform: {:?}", verify_response.platform);
                    } else {
                        println!("❌ Proof verification failed!");
                    }
                }
                Err(e) => {
                    println!("❌ Proof verification error: {}", e);
                }
            }

            // Demonstrate multiple proof generations
            println!("\n🔄 Generating additional proofs for comparison...");
            generate_multiple_proofs(&sdk, session_id, &signing_key, 3).await?;

        }
        Err(e) => {
            println!("❌ Proof generation failed: {}", e);

            if e.is_session_expired() {
                println!("💡 Hint: The session may have expired. Try authenticating again.");
            } else if e.is_auth_error() {
                println!("💡 Hint: Check your authentication credentials and session.");
            } else {
                println!("💡 Hint: Ensure the Kolme server is running and accessible.");
            }

            return Err(e);
        }
    }

    // Display platform information
    display_platform_info(platform);

    println!("\n🎉 Proof generation example completed!");
    Ok(())
}

fn display_proof_details(commitment: &SocialIdentityCommitment, proof: &kolme_zkp_sdk::ZkProof) {
    println!("\n📊 Proof Details:");
    println!("   ├─ Commitment:");
    println!("   │  ├─ Value: {}", hex::encode(commitment.commitment));
    println!("   │  └─ Platform: {:?}", commitment.platform);
    println!("   ├─ Proof:");
    println!("   │  ├─ Data size: {} bytes", proof.proof_data.len());
    println!("   │  ├─ Public inputs: {} items", proof.public_inputs.len());
    println!("   │  └─ Platform: {:?}", proof.platform);

    // Display first few bytes of proof data
    if proof.proof_data.len() >= 8 {
        let preview = &proof.proof_data[..8];
        println!("   └─ Proof preview: {}...", hex::encode(preview));
    }
}

async fn generate_multiple_proofs(
    sdk: &KolmeZkpSdk,
    session_id: &str,
    signing_key: &ed25519_dalek::SigningKey,
    count: usize,
) -> Result<()> {
    let mut timings = Vec::new();
    let mut commitments = Vec::new();

    for i in 1..=count {
        println!("   Generating proof {}/{}...", i, count);

        let signature = CryptoUtils::create_session_signature(signing_key, session_id)?;
        let start = std::time::Instant::now();

        match sdk.prove_identity(session_id, signature).await {
            Ok(response) => {
                let duration = start.elapsed();
                timings.push(duration);
                commitments.push(response.commitment.commitment);
                println!("   ✅ Proof {} completed in {:?}", i, duration);
            }
            Err(e) => {
                println!("   ❌ Proof {} failed: {}", i, e);
            }
        }
    }

    if !timings.is_empty() {
        let avg_time = timings.iter().sum::<std::time::Duration>() / timings.len() as u32;
        let min_time = timings.iter().min().unwrap();
        let max_time = timings.iter().max().unwrap();

        println!("\n📈 Performance Statistics:");
        println!("   ├─ Average time: {:?}", avg_time);
        println!("   ├─ Minimum time: {:?}", min_time);
        println!("   ├─ Maximum time: {:?}", max_time);
        println!("   └─ Total proofs: {}", timings.len());

        // Check if all commitments are unique (they should be due to nonces)
        let unique_commitments: std::collections::HashSet<_> = commitments.iter().collect();
        if unique_commitments.len() == commitments.len() {
            println!("✅ All commitments are unique (good randomness)");
        } else {
            println!("⚠️  Some commitments are identical (potential issue)");
        }
    }

    Ok(())
}

fn display_platform_info(platform: SocialPlatform) {
    println!("\n📱 Platform Information:");
    println!("   ├─ Name: {}", PlatformUtils::platform_display_name(platform));
    println!("   ├─ OAuth scope: {}", PlatformUtils::platform_oauth_scope(platform));
    println!("   ├─ Supports email: {}", PlatformUtils::platform_supports_email(platform));
    println!("   └─ User ID format: {}", PlatformUtils::platform_user_id_format(platform));
}
