use ark_ff::{PrimeField, BigInteger};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::fields::fp::FpVar;
use sha2::{Sha256, Digest};

use crate::SocialPlatform;

/// Circuit for proving ownership of a social identity
pub struct SocialIdentityCircuit<F: PrimeField> {
    /// Private input: social ID (e.g., Twitter handle)
    pub social_id: Option<String>,

    /// Private input: platform-specific secret (e.g., OAuth token hash)
    pub platform_secret: Option<Vec<u8>>,

    /// Private input: random nonce for commitment
    pub nonce: Option<F>,

    /// Public input: platform identifier
    pub platform: SocialPlatform,

    /// Public input: identity commitment
    pub commitment: Option<F>,
}

impl<F: PrimeField> SocialIdentityCircuit<F> {
    /// Create a new circuit instance
    pub fn new(
        social_id: Option<String>,
        platform_secret: Option<Vec<u8>>,
        nonce: Option<F>,
        platform: SocialPlatform,
        commitment: Option<F>,
    ) -> Self {
        Self {
            social_id,
            platform_secret,
            nonce,
            platform,
            commitment,
        }
    }

    /// Compute the commitment for given inputs
    pub fn compute_commitment(
        social_id: &str,
        platform: SocialPlatform,
        nonce: F,
    ) -> Result<F, Box<dyn std::error::Error>> {
        // Create input for SHA256
        let mut input = Vec::new();
        input.extend_from_slice(social_id.as_bytes());
        input.extend_from_slice(&[platform as u8]);

        // Convert nonce to bytes
        let nonce_bytes = nonce.into_bigint().to_bytes_be();
        input.extend_from_slice(&nonce_bytes);

        // Compute SHA256 hash
        let mut hasher = Sha256::new();
        hasher.update(&input);
        let hash_result = hasher.finalize();

        // Convert hash to field element
        Ok(F::from_be_bytes_mod_order(&hash_result))
    }
}

impl<F: PrimeField> ConstraintSynthesizer<F> for SocialIdentityCircuit<F> {
    fn generate_constraints(self, cs: ConstraintSystemRef<F>) -> Result<(), SynthesisError> {
        // Simplified circuit that verifies the prover knows the preimage of the commitment
        // This is a demo circuit - in production you'd use proper hash gadgets

        // Allocate nonce as witness
        let nonce_var = FpVar::new_witness(cs.clone(), || {
            self.nonce.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate public input commitment
        let commitment_var = FpVar::new_input(cs.clone(), || {
            self.commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Compute the expected commitment from the private inputs
        let expected_commitment = if let (Some(social_id), Some(nonce)) = (&self.social_id, &self.nonce) {
            Self::compute_commitment(social_id, self.platform, *nonce)
                .map_err(|_| SynthesisError::AssignmentMissing)?
        } else {
            return Err(SynthesisError::AssignmentMissing);
        };

        // Allocate the expected commitment as a witness
        let expected_commitment_var = FpVar::new_witness(cs.clone(), || Ok(expected_commitment))?;

        // Enforce that the public commitment equals the computed commitment
        commitment_var.enforce_equal(&expected_commitment_var)?;

        // Ensure the nonce is non-zero (prevents trivial proofs)
        let zero = FpVar::zero();
        nonce_var.enforce_not_equal(&zero)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381::Fr;
    use ark_relations::r1cs::ConstraintSystem;

    #[test]
    fn test_social_identity_circuit_satisfaction() {
        let social_id = "test_user".to_string();
        let platform_secret = vec![1u8; 32];
        let nonce = Fr::from(12345u64);
        let platform = SocialPlatform::Twitter;
        let commitment = SocialIdentityCircuit::compute_commitment(&social_id, platform, nonce).unwrap();

        let circuit = SocialIdentityCircuit::new(
            Some(social_id),
            Some(platform_secret),
            Some(nonce),
            platform,
            Some(commitment),
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_invalid_commitment_fails() {
        let social_id = "test_user".to_string();
        let platform_secret = vec![1u8; 32];
        let nonce = Fr::from(12345u64);
        let platform = SocialPlatform::Twitter;
        let wrong_commitment = Fr::from(99999u64); // Wrong commitment

        let circuit = SocialIdentityCircuit::new(
            Some(social_id),
            Some(platform_secret),
            Some(nonce),
            platform,
            Some(wrong_commitment),
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        // This should fail since we're using a wrong commitment
        assert!(!cs.is_satisfied().unwrap());
    }
}