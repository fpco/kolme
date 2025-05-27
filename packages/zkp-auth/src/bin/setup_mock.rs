//! Mock setup binary for generating test parameters quickly

use std::fs::{create_dir_all, File};
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔐 Kolme ZKP Mock Setup");
    println!("======================\n");
    
    println!("⚠️  WARNING: This generates MOCK parameters for testing only!");
    println!("   Do NOT use these parameters in production!\n");

    // Create output directory
    create_dir_all("data")?;

    println!("📊 Generating mock parameters...");
    
    // Create mock proving key (larger size to simulate real key)
    println!("   Creating mock proving_key.bin (50KB)...");
    let pk_data = vec![0x42u8; 50_000]; // 50KB of dummy data
    let mut pk_file = File::create("data/proving_key.bin")?;
    pk_file.write_all(&pk_data)?;
    
    // Create mock verifying key (smaller)
    println!("   Creating mock verifying_key.bin (2KB)...");
    let vk_data = vec![0x43u8; 2_000]; // 2KB of dummy data
    let mut vk_file = File::create("data/verifying_key.bin")?;
    vk_file.write_all(&vk_data)?;
    
    // Create parameter hash
    println!("   Creating parameters.hash...");
    let hash = "mock-parameters-hash-abc123def456";
    let mut hash_file = File::create("data/parameters.hash")?;
    writeln!(hash_file, "{}", hash)?;
    
    println!("\n✅ Mock setup complete!");
    println!("   Files created in data/");
    println!("\n📝 Note: To generate real parameters:");
    println!("   1. Fix the arkworks dependency issues");
    println!("   2. Run: cargo run --bin setup");
    
    Ok(())
}