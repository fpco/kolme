//! Trusted setup binary for generating proving and verifying keys

use ark_bls12_381::{Bls12_381, Fr};
use ark_serialize::CanonicalSerialize;
use ark_snark::SNARK;
use kolme_zkp_auth::circuits::SocialIdentityCircuit;
use std::fs::{create_dir_all, File};
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔐 Kolme ZKP Trusted Setup");
    println!("==========================\n");

    // Create output directory
    create_dir_all("data")?;

    println!("📊 Generating parameters for social identity circuit...");
    let start = std::time::Instant::now();

    // Create a dummy circuit for parameter generation
    let dummy_commitment = SocialIdentityCircuit::<Fr>::compute_commitment(
        "dummy_user",
        kolme_zkp_auth::SocialPlatform::Twitter,
        Fr::from(12345u64),
    )?;
    
    let circuit = SocialIdentityCircuit::<Fr> {
        social_id: Some("dummy_user".to_string()),
        platform: kolme_zkp_auth::SocialPlatform::Twitter,
        platform_secret: Some(vec![0u8; 32]),
        nonce: Some(Fr::from(12345u64)),
        commitment: Some(dummy_commitment),
    };

    // Generate parameters
    let rng = &mut ark_std::rand::thread_rng();
    let (pk, vk) = ark_groth16::Groth16::<Bls12_381>::circuit_specific_setup(circuit, rng)?;
    let generation_time = start.elapsed();

    println!("✅ Parameters generated in {:?}", generation_time);

    // Save proving key
    println!("\n📁 Saving proving key...");
    let mut pk_bytes = Vec::new();
    pk.serialize_compressed(&mut pk_bytes)?;
    let mut pk_file = File::create("data/proving_key.bin")?;
    pk_file.write_all(&pk_bytes)?;
    println!("   Size: {} bytes", pk_bytes.len());

    // Save verifying key
    println!("\n📁 Saving verifying key...");
    let mut vk_bytes = Vec::new();
    vk.serialize_compressed(&mut vk_bytes)?;
    let mut vk_file = File::create("data/verifying_key.bin")?;
    vk_file.write_all(&vk_bytes)?;
    println!("   Size: {} bytes", vk_bytes.len());

    // Generate and save a hash of the parameters for verification
    println!("\n🔍 Computing parameter hash...");
    let mut hasher = blake3::Hasher::new();
    hasher.update(&pk_bytes);
    hasher.update(&vk_bytes);
    
    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash.as_bytes());
    
    let mut hash_file = File::create("data/parameters.hash")?;
    writeln!(hash_file, "{}", hash_hex)?;
    
    println!("   Hash: {}", &hash_hex[..16]);

    // Print summary
    println!("\n✨ Trusted Setup Complete!");
    println!("========================");
    println!("Files created:");
    println!("  - data/proving_key.bin");
    println!("  - data/verifying_key.bin");
    println!("  - data/parameters.hash");
    println!("\n⚠️  IMPORTANT: These parameters should be generated");
    println!("   via a secure multi-party computation ceremony");
    println!("   for production use!");

    Ok(())
}