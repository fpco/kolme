use ark_bls12_381::{Bls12_381, Fr};
use ark_groth16::{Groth16, ProvingKey};
use ark_serialize::{CanonicalSerialize, CanonicalDeserialize};
use ark_std::rand::SeedableRng;
use ark_ff::{UniformRand, PrimeField, BigInteger};
use ark_snark::SNARK;

use crate::{
    circuits::SocialIdentityCircuit,
    errors::{Result, ZkpAuthError},
    SocialPlatform, ZkProof, SocialIdentityCommitment,
};

/// ZKP prover for social identity
pub struct Prover {
    proving_key: Option<ProvingKey<Bls12_381>>,
}

impl Prover {
    /// Create a new prover instance
    pub fn new() -> Self {
        Self {
            proving_key: None,
        }
    }

    /// Initialize with proving key
    pub fn with_proving_key(proving_key: ProvingKey<Bls12_381>) -> Self {
        Self {
            proving_key: Some(proving_key),
        }
    }

    /// Load proving key from bytes
    pub fn load_proving_key(&mut self, data: &[u8]) -> Result<()> {
        let pk = ProvingKey::<Bls12_381>::deserialize_compressed(data)
            .map_err(|e| ZkpAuthError::SerializationError { reason: e.to_string() })?;
        self.proving_key = Some(pk);
        Ok(())
    }

    /// Set proving key directly
    pub fn set_proving_key(&mut self, pk: ProvingKey<Bls12_381>) {
        self.proving_key = Some(pk);
    }

    /// Generate a proof of social identity ownership
    pub async fn prove_identity(
        &self,
        social_id: &str,
        platform: SocialPlatform,
        platform_secret: Vec<u8>,
    ) -> Result<(SocialIdentityCommitment, ZkProof)> {
        let proving_key = self.proving_key.as_ref()
            .ok_or(ZkpAuthError::TrustedSetupNotInitialized)?;

        // Generate random nonce
        let mut rng = ark_std::rand::rngs::StdRng::from_entropy();
        let nonce = Fr::rand(&mut rng);

        // Compute commitment
        let commitment = SocialIdentityCircuit::compute_commitment(social_id, platform, nonce)
            .map_err(|e| ZkpAuthError::ProofGenerationFailed { reason: e.to_string() })?;

        eprintln!("Debug: Generated commitment field: {:?}", commitment);
        eprintln!("Debug: social_id: {}, platform: {:?}, nonce: {:?}", social_id, platform, nonce);

        // Create circuit
        let circuit = SocialIdentityCircuit::new(
            Some(social_id.to_string()),
            Some(platform_secret),
            Some(nonce),
            platform,
            Some(commitment),
        );

        // Generate proof
        let proof = Groth16::<Bls12_381>::prove(proving_key, circuit, &mut rng)
            .map_err(|e| ZkpAuthError::ProofGenerationFailed { reason: e.to_string() })?;

        // Serialize proof
        let mut proof_bytes = Vec::new();
        proof.serialize_compressed(&mut proof_bytes)?;

        // Convert commitment field element to 32-byte array using big-endian representation
        let commitment_bytes = commitment.into_bigint().to_bytes_be();
        let mut commitment_array = [0u8; 32];

        // Field elements are typically 32 bytes for BLS12-381 Fr
        if commitment_bytes.len() <= 32 {
            // Right-align the bytes (big-endian)
            let offset = 32 - commitment_bytes.len();
            commitment_array[offset..].copy_from_slice(&commitment_bytes);
        } else {
            // Take the last 32 bytes if somehow larger
            commitment_array.copy_from_slice(&commitment_bytes[commitment_bytes.len()-32..]);
        }

        eprintln!("Debug: Commitment field converted to 32-byte array");

        let identity_commitment = SocialIdentityCommitment {
            commitment: commitment_array,
            platform,
        };

        let zk_proof = ZkProof {
            proof_data: proof_bytes,
            public_inputs: vec![commitment_array],
            platform,
        };

        Ok((identity_commitment, zk_proof))
    }

    /// Prove ownership of an existing social identity
    pub async fn prove_ownership(
        &self,
        social_id: &str,
        platform: SocialPlatform,
        platform_secret: Vec<u8>,
        challenge: [u8; 32],
    ) -> Result<ZkProof> {
        // For ownership proofs, we use the challenge as part of the nonce
        let mut nonce_bytes = [0u8; 32];
        nonce_bytes.copy_from_slice(&challenge);
        let nonce = Fr::from_be_bytes_mod_order(&nonce_bytes);

        let proving_key = self.proving_key.as_ref()
            .ok_or(ZkpAuthError::TrustedSetupNotInitialized)?;

        // Compute commitment with challenge
        let commitment = SocialIdentityCircuit::compute_commitment(social_id, platform, nonce)
            .map_err(|e| ZkpAuthError::ProofGenerationFailed { reason: e.to_string() })?;

        // Create circuit
        let circuit = SocialIdentityCircuit::new(
            Some(social_id.to_string()),
            Some(platform_secret),
            Some(nonce),
            platform,
            Some(commitment),
        );

        // Generate proof
        let mut rng = ark_std::rand::rngs::StdRng::from_entropy();
        let proof = Groth16::<Bls12_381>::prove(proving_key, circuit, &mut rng)
            .map_err(|e| ZkpAuthError::ProofGenerationFailed { reason: e.to_string() })?;

        // Serialize proof
        let mut proof_bytes = Vec::new();
        proof.serialize_compressed(&mut proof_bytes)?;

        // Convert commitment to 32-byte array using big-endian representation
        let commitment_bigint_bytes = commitment.into_bigint().to_bytes_be();
        let mut commitment_bytes = [0u8; 32];
        if commitment_bigint_bytes.len() <= 32 {
            let offset = 32 - commitment_bigint_bytes.len();
            commitment_bytes[offset..].copy_from_slice(&commitment_bigint_bytes);
        } else {
            commitment_bytes.copy_from_slice(&commitment_bigint_bytes[commitment_bigint_bytes.len()-32..]);
        }

        Ok(ZkProof {
            proof_data: proof_bytes,
            public_inputs: vec![commitment_bytes, challenge],
            platform,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prover_creation() {
        let prover = Prover::new();
        assert!(prover.proving_key.is_none());
    }

    #[test]
    fn test_prover_load_key() {
        let mut prover = Prover::new();
        // Test with invalid data should fail
        let invalid_data = vec![0u8; 10];
        assert!(prover.load_proving_key(&invalid_data).is_err());
    }
}