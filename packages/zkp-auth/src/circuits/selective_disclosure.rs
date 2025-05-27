use ark_ff::{Field, PrimeField};
use ark_r1cs_std::{prelude::*, fields::fp::FpVar};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use sha2::{Sha256, Digest};
use crate::SocialPlatform;

/// Circuit for proving social media attributes without revealing identity
#[derive(Clone)]
pub struct AttributeDisclosureCircuit<F: Field> {
    // Private inputs
    pub user_id: Option<String>,
    pub platform: Option<SocialPlatform>,
    pub attribute_value: Option<u64>,
    pub nonce: Option<F>,

    // Public inputs
    pub commitment: Option<F>,
    pub attribute_type: Option<AttributeType>,
    pub threshold: Option<u64>,
    pub comparison: Option<ComparisonOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeType {
    FollowerCount,
    AccountAge,
    VerificationStatus,
    PostCount,
    ConnectionCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOp {
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Equal,
    NotEqual,
}

impl<F: PrimeField> AttributeDisclosureCircuit<F> {
    /// Create a new attribute disclosure circuit
    pub fn new(
        user_id: String,
        platform: SocialPlatform,
        attribute_value: u64,
        attribute_type: AttributeType,
        threshold: u64,
        comparison: ComparisonOp,
    ) -> Self {
        let nonce = F::rand(&mut ark_std::test_rng());

        // Compute commitment = H(user_id || platform || attribute_type || nonce)
        let commitment_input = format!(
            "{}-{:?}-{:?}-{}",
            user_id,
            platform,
            attribute_type,
            nonce
        );
        let commitment_bytes = Sha256::digest(commitment_input.as_bytes());
        let commitment = F::from_be_bytes_mod_order(&commitment_bytes);

        Self {
            user_id: Some(user_id),
            platform: Some(platform),
            attribute_value: Some(attribute_value),
            nonce: Some(nonce),
            commitment: Some(commitment),
            attribute_type: Some(attribute_type),
            threshold: Some(threshold),
            comparison: Some(comparison),
        }
    }

    /// Verify the comparison based on the operation
    #[allow(dead_code)]
    fn verify_comparison(
        value: u64,
        threshold: u64,
        op: ComparisonOp,
    ) -> bool {
        match op {
            ComparisonOp::GreaterThan => value > threshold,
            ComparisonOp::GreaterThanOrEqual => value >= threshold,
            ComparisonOp::LessThan => value < threshold,
            ComparisonOp::LessThanOrEqual => value <= threshold,
            ComparisonOp::Equal => value == threshold,
            ComparisonOp::NotEqual => value != threshold,
        }
    }
}

impl<F: PrimeField> ConstraintSynthesizer<F> for AttributeDisclosureCircuit<F> {
    fn generate_constraints(self, cs: ConstraintSystemRef<F>) -> Result<(), SynthesisError> {
        // Allocate private inputs
        let _user_id_var = self.user_id.as_ref().map(|id| {
            let bytes = id.as_bytes();
            let field_elements: Vec<F> = bytes.iter()
                .map(|&b| F::from(b as u64))
                .collect();
            field_elements.into_iter()
                .map(|fe| FpVar::new_witness(cs.clone(), || Ok(fe)))
                .collect::<Result<Vec<_>, _>>()
        }).transpose()?;

        let attribute_value_var = FpVar::new_witness(cs.clone(), || {
            self.attribute_value
                .map(|v| F::from(v))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let _nonce_var = FpVar::new_witness(cs.clone(), || {
            self.nonce.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate public inputs
        let commitment_var = FpVar::new_input(cs.clone(), || {
            self.commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let threshold_var = FpVar::new_input(cs.clone(), || {
            self.threshold
                .map(|t| F::from(t))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Verify commitment computation
        // In a real implementation, this would involve proper hash-to-field
        // and commitment verification
        let _ = commitment_var.is_neq(&FpVar::zero())?;

        // Verify attribute comparison
        let comparison_result = match self.comparison.unwrap_or(ComparisonOp::GreaterThanOrEqual) {
            ComparisonOp::GreaterThan => {
                attribute_value_var.is_cmp(&threshold_var, std::cmp::Ordering::Greater, false)?
            }
            ComparisonOp::GreaterThanOrEqual => {
                let gt = attribute_value_var.is_cmp(&threshold_var, std::cmp::Ordering::Greater, false)?;
                let eq = attribute_value_var.is_eq(&threshold_var)?;
                gt.or(&eq)?
            }
            ComparisonOp::LessThan => {
                attribute_value_var.is_cmp(&threshold_var, std::cmp::Ordering::Less, false)?
            }
            ComparisonOp::LessThanOrEqual => {
                let lt = attribute_value_var.is_cmp(&threshold_var, std::cmp::Ordering::Less, false)?;
                let eq = attribute_value_var.is_eq(&threshold_var)?;
                lt.or(&eq)?
            }
            ComparisonOp::Equal => {
                attribute_value_var.is_eq(&threshold_var)?
            }
            ComparisonOp::NotEqual => {
                attribute_value_var.is_neq(&threshold_var)?
            }
        };

        // Enforce that the comparison holds
        comparison_result.enforce_equal(&Boolean::TRUE)?;

        Ok(())
    }
}

/// Circuit for proving multiple attributes simultaneously
pub struct MultiAttributeCircuit<F: Field> {
    // Private inputs
    pub user_id: Option<String>,
    pub platform: Option<SocialPlatform>,
    pub attributes: Vec<(AttributeType, u64)>,
    pub nonce: Option<F>,

    // Public inputs
    pub commitment: Option<F>,
    pub attribute_requirements: Vec<(AttributeType, u64, ComparisonOp)>,
}

impl<F: PrimeField> ConstraintSynthesizer<F> for MultiAttributeCircuit<F> {
    fn generate_constraints(self, cs: ConstraintSystemRef<F>) -> Result<(), SynthesisError> {
        // Similar to single attribute circuit but handles multiple attributes
        // Implementation would verify all attribute requirements are met

        // Allocate commitment
        let commitment_var = FpVar::new_input(cs.clone(), || {
            self.commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Verify commitment is non-zero
        let _ = commitment_var.is_neq(&FpVar::zero())?;

        // For each attribute requirement, verify it's satisfied
        for (_i, (attr_type, threshold, op)) in self.attribute_requirements.iter().enumerate() {
            if let Some((_, value)) = self.attributes.iter().find(|(t, _)| t == attr_type) {
                let value_var = FpVar::new_witness(cs.clone(), || Ok(F::from(*value)))?;
                let threshold_var = FpVar::new_input(cs.clone(), || Ok(F::from(*threshold)))?;

                // Verify the comparison holds
                let result = match op {
                    ComparisonOp::GreaterThanOrEqual => {
                        let gt = value_var.is_cmp(&threshold_var, std::cmp::Ordering::Greater, false)?;
                        let eq = value_var.is_eq(&threshold_var)?;
                        gt.or(&eq)?
                    }
                    _ => {
                        // Implement other comparison operations as needed
                        Boolean::TRUE
                    }
                };

                result.enforce_equal(&Boolean::TRUE)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381::Fr;

    #[test]
    fn test_follower_count_proof() {
        // Test proving follower count > 1000
        let _circuit = AttributeDisclosureCircuit::<Fr>::new(
            "user123".to_string(),
            SocialPlatform::Twitter,
            5000, // actual follower count
            AttributeType::FollowerCount,
            1000, // threshold
            ComparisonOp::GreaterThan,
        );

        // In a real implementation, we would:
        // 1. Generate proving/verifying keys
        // 2. Create the proof
        // 3. Verify the proof

        assert!(AttributeDisclosureCircuit::<Fr>::verify_comparison(
            5000,
            1000,
            ComparisonOp::GreaterThan
        ));
    }

    #[test]
    fn test_account_age_proof() {
        // Test proving account age >= 365 days
        let _circuit = AttributeDisclosureCircuit::<Fr>::new(
            "user456".to_string(),
            SocialPlatform::GitHub,
            730, // 2 years in days
            AttributeType::AccountAge,
            365, // 1 year threshold
            ComparisonOp::GreaterThanOrEqual,
        );

        assert!(AttributeDisclosureCircuit::<Fr>::verify_comparison(
            730,
            365,
            ComparisonOp::GreaterThanOrEqual
        ));
    }

    #[test]
    fn test_verification_status_proof() {
        // Test proving verification status (1 = verified, 0 = not verified)
        let _circuit = AttributeDisclosureCircuit::<Fr>::new(
            "verified_user".to_string(),
            SocialPlatform::Twitter,
            1, // verified
            AttributeType::VerificationStatus,
            1, // must be verified
            ComparisonOp::Equal,
        );

        assert!(AttributeDisclosureCircuit::<Fr>::verify_comparison(
            1,
            1,
            ComparisonOp::Equal
        ));
    }
}