//! Integration tests for the complete ZKP flow

use ark_bls12_381::Fr;
use ark_snark::SNARK;
use kolme_zkp_auth::{
    Prover, Verifier, SocialPlatform, SocialIdentityCommitment,
    circuits::SocialIdentityCircuit,
    keys::KeyManager,
};
use tempfile::tempdir;

#[test]
fn test_complete_zkp_flow() {
    // Create temporary directory for test parameters
    let temp_dir = tempdir().unwrap();
    let key_manager = KeyManager::with_dir(temp_dir.path());

    println!("🔧 Generating test parameters...");

    // Generate test parameters
    let rng = &mut ark_std::rand::thread_rng();

    // Create dummy circuit for setup with dummy values that match the structure
    let dummy_social_id = "dummy_user";
    let dummy_nonce = Fr::from(1u64);
    let dummy_commitment = SocialIdentityCircuit::<Fr>::compute_commitment(
        dummy_social_id,
        SocialPlatform::Twitter,
        dummy_nonce,
    ).unwrap();

    let circuit = SocialIdentityCircuit::<Fr> {
        social_id: Some(dummy_social_id.to_string()),
        platform: SocialPlatform::Twitter,
        platform_secret: Some(vec![0u8; 32]),
        nonce: Some(dummy_nonce),
        commitment: Some(dummy_commitment),
    };

    // Generate parameters
    let (pk, vk) = ark_groth16::Groth16::<ark_bls12_381::Bls12_381>::circuit_specific_setup(
        circuit,
        rng,
    ).unwrap();

    // Save parameters
    key_manager.save_proving_key(&pk).unwrap();
    key_manager.save_verifying_key(&vk).unwrap();

    println!("✅ Parameters generated and saved");

    // Test proof generation and verification
    println!("\n🔐 Testing proof generation...");

    // Create prover and verifier
    let mut prover = Prover::new();
    prover.set_proving_key(pk.clone());

    let mut verifier = Verifier::new();
    verifier.set_verifying_key(vk.clone());

    // Generate proof
    let social_id = "alice_wonderland";
    let platform = SocialPlatform::Twitter;
    let platform_secret = b"mock_oauth_token".to_vec();

    let (commitment, proof) = tokio_test::block_on(
        prover.prove_identity(social_id, platform, platform_secret)
    ).unwrap();

    println!("✅ Proof generated successfully");
    println!("   Commitment: {:?}", hex::encode(&commitment.commitment[..8]));
    println!("   Proof size: {} bytes", proof.proof_data.len());

    // Verify proof
    println!("\n🔍 Verifying proof...");

    match verifier.verify_identity_proof(&commitment, &proof) {
        Ok(is_valid) => {
            if !is_valid {
                panic!("Proof verification returned false!");
            }
        }
        Err(e) => {
            panic!("Proof verification error: {:?}", e);
        }
    }
    println!("✅ Proof verified successfully!");

    // Test invalid proof
    println!("\n❌ Testing invalid proof detection...");

    let mut invalid_proof = proof.clone();
    // Corrupt the proof in a way that still allows deserialization but makes it invalid
    if invalid_proof.proof_data.len() > 10 {
        invalid_proof.proof_data[10] ^= 0x01; // Small corruption that won't break deserialization
    }

    let is_invalid = match verifier.verify_identity_proof(&commitment, &invalid_proof) {
        Ok(valid) => !valid, // If verification succeeds, the proof should be invalid
        Err(_) => true, // If verification fails due to format error, that's also detecting invalidity
    };
    assert!(is_invalid, "Invalid proof was accepted!");

    println!("✅ Invalid proof correctly rejected");

    // Test wrong commitment
    let wrong_commitment = SocialIdentityCommitment {
        commitment: [0xFF; 32],
        platform,
    };

    let is_wrong = verifier.verify_identity_proof(&wrong_commitment, &proof).unwrap();
    assert!(!is_wrong, "Wrong commitment was accepted!");

    println!("✅ Wrong commitment correctly rejected");

    println!("\n🎉 All ZKP flow tests passed!");
}

#[test]
fn test_different_platforms() {
    let platforms = vec![
        SocialPlatform::Twitter,
        SocialPlatform::GitHub,
        SocialPlatform::Discord,
        SocialPlatform::Google,
    ];

    for platform in platforms {
        println!("\n🧪 Testing platform: {:?}", platform);

        // Compute commitment
        let commitment = SocialIdentityCircuit::<Fr>::compute_commitment(
            "test_user",
            platform,
            Fr::from(9876u64),
        ).unwrap();

        // Verify commitment is non-zero
        assert_ne!(commitment, Fr::from(0u64));

        println!("✅ Commitment generated for {:?}", platform);
    }
}

#[test]
fn test_commitment_uniqueness() {
    let platform = SocialPlatform::GitHub;

    // Same inputs should produce same commitment
    let commitment1 = SocialIdentityCircuit::<Fr>::compute_commitment(
        "developer123",
        platform,
        Fr::from(1000u64),
    ).unwrap();

    let commitment2 = SocialIdentityCircuit::<Fr>::compute_commitment(
        "developer123",
        platform,
        Fr::from(1000u64),
    ).unwrap();

    assert_eq!(commitment1, commitment2, "Same inputs produced different commitments");

    // Different inputs should produce different commitments
    let commitment3 = SocialIdentityCircuit::<Fr>::compute_commitment(
        "developer456", // Different user
        platform,
        Fr::from(1000u64),
    ).unwrap();

    assert_ne!(commitment1, commitment3, "Different users produced same commitment");

    let commitment4 = SocialIdentityCircuit::<Fr>::compute_commitment(
        "developer123",
        platform,
        Fr::from(2000u64), // Different nonce
    ).unwrap();

    assert_ne!(commitment1, commitment4, "Different nonces produced same commitment");

    println!("✅ Commitment uniqueness verified");
}