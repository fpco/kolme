//! ZKP proof verification example
//!
//! This example demonstrates how to verify zero-knowledge proofs
//! using the Kolme ZKP SDK, including batch verification and
//! performance testing.
//!
//! Run with: cargo run --example proof_verification

use kolme_zkp_sdk::{
    KolmeZkpSdk, KolmeZkpConfig, SocialPlatform, SocialIdentityCommitment, ZkProof,
    VerifyProofRequest, VerifyProofResponse, ValidationUtils, Result,
};
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("🔍 Kolme ZKP SDK - Proof Verification Example");
    println!("=============================================");

    // Create SDK instance
    let config = KolmeZkpConfig::new("http://localhost:8080")
        .with_timeout(60)
        .with_header("X-Example", "proof-verification");

    let sdk = KolmeZkpSdk::new(config)?;

    // Example 1: Verify a valid proof
    println!("\n📋 Example 1: Valid Proof Verification");
    verify_valid_proof(&sdk).await?;

    // Example 2: Verify an invalid proof
    println!("\n📋 Example 2: Invalid Proof Detection");
    verify_invalid_proof(&sdk).await?;

    // Example 3: Batch verification
    println!("\n📋 Example 3: Batch Proof Verification");
    batch_verification(&sdk).await?;

    // Example 4: Performance testing
    println!("\n📋 Example 4: Verification Performance Testing");
    performance_testing(&sdk).await?;

    // Example 5: Cross-platform verification
    println!("\n📋 Example 5: Cross-Platform Verification");
    cross_platform_verification(&sdk).await?;

    println!("\n🎉 Proof verification examples completed!");
    Ok(())
}

async fn verify_valid_proof(sdk: &KolmeZkpSdk) -> Result<()> {
    println!("   Creating a valid proof for verification...");

    // Create a sample commitment (in real usage, this would come from proof generation)
    let commitment = SocialIdentityCommitment {
        commitment: [1u8; 32], // Sample commitment
        platform: SocialPlatform::GitHub,
    };

    // Validate commitment format
    ValidationUtils::validate_commitment(&commitment.commitment)?;

    // Create a sample proof (in real usage, this would come from proof generation)
    let proof = ZkProof {
        proof_data: vec![0u8; 192], // Sample proof data (typical Groth16 proof size)
        public_inputs: vec![[1u8; 32]], // Sample public input
        platform: SocialPlatform::GitHub,
    };

    let verify_request = VerifyProofRequest {
        commitment: commitment.clone(),
        proof: proof.clone(),
        platform: SocialPlatform::GitHub,
    };

    println!("   Verifying proof...");
    let start = Instant::now();

    match sdk.verify_proof(verify_request).await {
        Ok(response) => {
            let duration = start.elapsed();
            display_verification_result(&response, duration);

            if response.valid {
                println!("   ✅ Proof verification successful!");
            } else {
                println!("   ❌ Proof verification failed (expected for sample data)");
            }
        }
        Err(e) => {
            println!("   ❌ Verification error: {}", e);
        }
    }

    Ok(())
}

async fn verify_invalid_proof(sdk: &KolmeZkpSdk) -> Result<()> {
    println!("   Creating an obviously invalid proof...");

    // Create an invalid commitment (all zeros)
    let invalid_commitment = SocialIdentityCommitment {
        commitment: [0u8; 32], // Invalid: all zeros
        platform: SocialPlatform::Twitter,
    };

    // Create an invalid proof
    let invalid_proof = ZkProof {
        proof_data: vec![], // Invalid: empty proof data
        public_inputs: vec![], // Invalid: no public inputs
        platform: SocialPlatform::Twitter,
    };

    let verify_request = VerifyProofRequest {
        commitment: invalid_commitment,
        proof: invalid_proof,
        platform: SocialPlatform::Twitter,
    };

    println!("   Verifying invalid proof...");
    let start = Instant::now();

    match sdk.verify_proof(verify_request).await {
        Ok(response) => {
            let duration = start.elapsed();
            display_verification_result(&response, duration);

            if !response.valid {
                println!("   ✅ Invalid proof correctly rejected!");
            } else {
                println!("   ⚠️  Invalid proof was accepted (unexpected)");
            }
        }
        Err(e) => {
            println!("   ✅ Verification correctly failed: {}", e);
        }
    }

    Ok(())
}

async fn batch_verification(sdk: &KolmeZkpSdk) -> Result<()> {
    let batch_size = 5;
    println!("   Verifying {} proofs in batch...", batch_size);

    let mut results = Vec::new();
    let mut total_time = std::time::Duration::ZERO;

    for i in 1..=batch_size {
        // Create sample proof for each iteration
        let commitment = SocialIdentityCommitment {
            commitment: [i as u8; 32], // Different commitment for each proof
            platform: SocialPlatform::Discord,
        };

        let proof = ZkProof {
            proof_data: vec![i as u8; 192], // Different proof data
            public_inputs: vec![[i as u8; 32]],
            platform: SocialPlatform::Discord,
        };

        let verify_request = VerifyProofRequest {
            commitment,
            proof,
            platform: SocialPlatform::Discord,
        };

        println!("     Verifying proof {}/{}...", i, batch_size);
        let start = Instant::now();

        match sdk.verify_proof(verify_request).await {
            Ok(response) => {
                let duration = start.elapsed();
                total_time += duration;
                results.push((i, response.valid, duration));

                if response.valid {
                    println!("     ✅ Proof {} verified", i);
                } else {
                    println!("     ❌ Proof {} rejected", i);
                }
            }
            Err(e) => {
                println!("     ❌ Proof {} error: {}", i, e);
                results.push((i, false, start.elapsed()));
            }
        }
    }

    // Display batch results
    println!("\n   📊 Batch Verification Results:");
    println!("     ├─ Total proofs: {}", batch_size);
    println!("     ├─ Valid proofs: {}", results.iter().filter(|(_, valid, _)| *valid).count());
    println!("     ├─ Invalid proofs: {}", results.iter().filter(|(_, valid, _)| !*valid).count());
    println!("     ├─ Total time: {:?}", total_time);
    println!("     └─ Average time: {:?}", total_time / batch_size as u32);

    Ok(())
}

async fn performance_testing(sdk: &KolmeZkpSdk) -> Result<()> {
    let test_count = 10;
    println!("   Running {} verification performance tests...", test_count);

    let mut timings = Vec::new();

    for i in 1..=test_count {
        let commitment = SocialIdentityCommitment {
            commitment: [(i % 256) as u8; 32],
            platform: SocialPlatform::Google,
        };

        let proof = ZkProof {
            proof_data: vec![(i % 256) as u8; 192],
            public_inputs: vec![[(i % 256) as u8; 32]],
            platform: SocialPlatform::Google,
        };

        let verify_request = VerifyProofRequest {
            commitment,
            proof,
            platform: SocialPlatform::Google,
        };

        let start = Instant::now();
        let _ = sdk.verify_proof(verify_request).await; // Ignore result for timing
        let duration = start.elapsed();

        timings.push(duration);

        if i % 3 == 0 {
            print!(".");
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
        }
    }

    println!(); // New line after dots

    // Calculate statistics
    let total_time: std::time::Duration = timings.iter().sum();
    let avg_time = total_time / timings.len() as u32;
    let min_time = *timings.iter().min().unwrap();
    let max_time = *timings.iter().max().unwrap();

    // Calculate percentiles
    let mut sorted_timings = timings.clone();
    sorted_timings.sort();
    let p50 = sorted_timings[sorted_timings.len() / 2];
    let p95 = sorted_timings[(sorted_timings.len() * 95) / 100];

    println!("\n   📈 Performance Statistics:");
    println!("     ├─ Total verifications: {}", test_count);
    println!("     ├─ Average time: {:?}", avg_time);
    println!("     ├─ Minimum time: {:?}", min_time);
    println!("     ├─ Maximum time: {:?}", max_time);
    println!("     ├─ 50th percentile: {:?}", p50);
    println!("     ├─ 95th percentile: {:?}", p95);
    println!("     └─ Throughput: {:.2} verifications/sec", test_count as f64 / total_time.as_secs_f64());

    Ok(())
}

async fn cross_platform_verification(sdk: &KolmeZkpSdk) -> Result<()> {
    println!("   Testing verification across different platforms...");

    let platforms = [
        SocialPlatform::Twitter,
        SocialPlatform::GitHub,
        SocialPlatform::Discord,
        SocialPlatform::Google,
    ];

    for (i, platform) in platforms.iter().enumerate() {
        println!("     Testing platform: {:?}", platform);

        let commitment = SocialIdentityCommitment {
            commitment: [(i + 1) as u8; 32],
            platform: *platform,
        };

        let proof = ZkProof {
            proof_data: vec![(i + 1) as u8; 192],
            public_inputs: vec![[(i + 1) as u8; 32]],
            platform: *platform,
        };

        let verify_request = VerifyProofRequest {
            commitment,
            proof,
            platform: *platform,
        };

        match sdk.verify_proof(verify_request).await {
            Ok(response) => {
                if response.platform == Some(*platform) {
                    println!("     ✅ Platform consistency verified");
                } else {
                    println!("     ⚠️  Platform mismatch detected");
                }
            }
            Err(e) => {
                println!("     ❌ Platform {:?} error: {}", platform, e);
            }
        }
    }

    Ok(())
}

fn display_verification_result(response: &VerifyProofResponse, duration: std::time::Duration) {
    println!("   📊 Verification Result:");
    println!("     ├─ Valid: {}", response.valid);
    println!("     ├─ Platform: {:?}", response.platform);
    println!("     └─ Time: {:?}", duration);
}
