//! Simplified hash implementation for ZKP circuits

use ark_ff::PrimeField;
use sha2::{Sha256, Digest};

/// Simple SHA256-based commitment (not ideal for ZK but works)
pub fn compute_commitment<F: PrimeField>(
    social_id: &str,
    platform: crate::SocialPlatform,
    nonce: F,
) -> F {
    let mut hasher = Sha256::new();
    hasher.update(social_id.as_bytes());
    hasher.update(platform.as_str().as_bytes());
    hasher.update(&nonce.to_string().as_bytes());
    
    let hash = hasher.finalize();
    F::from_be_bytes_mod_order(&hash)
}