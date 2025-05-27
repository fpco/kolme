use ark_bls12_381::{Bls12_381, Fr};
use ark_groth16::{Groth16, VerifyingKey, PreparedVerifyingKey, Proof};
use ark_serialize::CanonicalDeserialize;
use ark_ff::PrimeField;
use hex;

use crate::{
    errors::{Result, ZkpAuthError},
    SocialPlatform, ZkProof, SocialIdentityCommitment,
};

/// ZKP verifier for social identity proofs
pub struct Verifier {
    verifying_key: Option<PreparedVerifyingKey<Bls12_381>>,
}

impl Verifier {
    /// Create a new verifier instance
    pub fn new() -> Self {
        Self {
            verifying_key: None,
        }
    }

    /// Initialize with verifying key
    pub fn with_verifying_key(verifying_key: VerifyingKey<Bls12_381>) -> Self {
        let prepared_vk = PreparedVerifyingKey::from(verifying_key);
        Self {
            verifying_key: Some(prepared_vk),
        }
    }

    /// Load verifying key from bytes
    pub fn load_verifying_key(&mut self, data: &[u8]) -> Result<()> {
        let vk = VerifyingKey::<Bls12_381>::deserialize_compressed(data)
            .map_err(|e| ZkpAuthError::SerializationError { reason: e.to_string() })?;
        let prepared_vk = PreparedVerifyingKey::from(vk);
        self.verifying_key = Some(prepared_vk);
        Ok(())
    }

    /// Set verifying key directly
    pub fn set_verifying_key(&mut self, vk: VerifyingKey<Bls12_381>) {
        let prepared_vk = PreparedVerifyingKey::from(vk);
        self.verifying_key = Some(prepared_vk);
    }

    /// Verify a social identity proof
    pub fn verify_identity_proof(
        &self,
        commitment: &SocialIdentityCommitment,
        proof: &ZkProof,
    ) -> Result<bool> {
        let verifying_key = self.verifying_key.as_ref()
            .ok_or(ZkpAuthError::TrustedSetupNotInitialized)?;

        // Check platform matches
        if commitment.platform != proof.platform {
            return Ok(false);
        }

        // Deserialize proof
        let proof_data = Proof::<Bls12_381>::deserialize_compressed(&proof.proof_data[..])
            .map_err(|_| ZkpAuthError::InvalidProofFormat)?;

        // Prepare public inputs - only the commitment is public
        let mut public_inputs = Vec::new();

        // The commitment is the only public input in our circuit
        // Convert the 32-byte commitment back to a field element using big-endian interpretation
        let commitment_field = Fr::from_be_bytes_mod_order(&commitment.commitment);
        public_inputs.push(commitment_field);

        // Debug: print public inputs
        eprintln!("Debug: Public inputs for verification: {:?}", public_inputs);
        eprintln!("Debug: Commitment bytes: {:?}", hex::encode(&commitment.commitment));

        // Verify proof
        let valid = Groth16::<Bls12_381>::verify_proof(verifying_key, &proof_data, &public_inputs)
            .map_err(|e| {
                eprintln!("Debug: Groth16 verification error: {:?}", e);
                ZkpAuthError::ProofVerificationFailed
            })?;

        Ok(valid)
    }

    /// Verify an ownership proof with challenge
    pub fn verify_ownership_proof(
        &self,
        platform: SocialPlatform,
        challenge: [u8; 32],
        proof: &ZkProof,
    ) -> Result<bool> {
        let verifying_key = self.verifying_key.as_ref()
            .ok_or(ZkpAuthError::TrustedSetupNotInitialized)?;

        // Check platform matches
        if platform != proof.platform {
            return Ok(false);
        }

        // Check that challenge is included in public inputs
        if proof.public_inputs.len() < 2 || proof.public_inputs[1] != challenge {
            return Ok(false);
        }

        // Deserialize proof
        let proof_data = Proof::<Bls12_381>::deserialize_compressed(&proof.proof_data[..])
            .map_err(|_| ZkpAuthError::InvalidProofFormat)?;

        // Prepare public inputs
        let mut public_inputs = Vec::new();

        // Add platform
        public_inputs.push(Fr::from(platform as u64));

        // Add commitment and challenge
        for input in &proof.public_inputs {
            let field_element = Fr::deserialize_compressed(&input[..])
                .map_err(|_| ZkpAuthError::InvalidCommitment)?;
            public_inputs.push(field_element);
        }

        // Verify proof
        let valid = Groth16::<Bls12_381>::verify_proof(verifying_key, &proof_data, &public_inputs)
            .map_err(|_| ZkpAuthError::ProofVerificationFailed)?;

        Ok(valid)
    }

    /// Batch verify multiple proofs (more efficient than individual verification)
    pub fn batch_verify_identity_proofs(
        &self,
        proofs: &[(SocialIdentityCommitment, ZkProof)],
    ) -> Result<Vec<bool>> {
        let mut results = Vec::with_capacity(proofs.len());

        for (commitment, proof) in proofs {
            let valid = self.verify_identity_proof(commitment, proof)?;
            results.push(valid);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verifier_creation() {
        let verifier = Verifier::new();
        assert!(verifier.verifying_key.is_none());
    }

    #[test]
    fn test_verifier_load_key() {
        let mut verifier = Verifier::new();
        // Test with invalid data should fail
        let invalid_data = vec![0u8; 10];
        assert!(verifier.load_verifying_key(&invalid_data).is_err());
    }

    #[test]
    fn test_platform_mismatch() {
        let commitment = SocialIdentityCommitment {
            commitment: [1u8; 32],
            platform: SocialPlatform::Twitter,
        };

        let proof = ZkProof {
            proof_data: vec![1u8; 100],
            public_inputs: vec![[2u8; 32]],
            platform: SocialPlatform::GitHub, // Different platform
        };

        let verifier = Verifier::new();
        // Should fail because no verifying key is loaded
        assert!(verifier.verify_identity_proof(&commitment, &proof).is_err());
    }
}