//! Poseidon hash implementation for ZKP circuits

use ark_ff::PrimeField;
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::*;
use ark_relations::r1cs::{ConstraintSystemRef, SynthesisError};
use ark_sponge::poseidon::{PoseidonConfig, PoseidonSponge};
use ark_sponge::{Absorb, CryptographicSponge};

/// Poseidon hash parameters for BLS12-381
pub fn poseidon_parameters<F: PrimeField + Absorb>() -> PoseidonConfig<F> {
    // Parameters for 2-to-1 hash
    // In production, these should be generated through a secure process
    PoseidonConfig {
        full_rounds: 8,
        partial_rounds: 57,
        alpha: 5,
        mds: generate_mds_matrix::<F>(3),
        ark: generate_ark_constants::<F>(3, 8, 57),
        rate: 2,
        capacity: 1,
    }
}

/// Generate MDS matrix for Poseidon
fn generate_mds_matrix<F: PrimeField>(size: usize) -> Vec<Vec<F>> {
    // Simplified MDS matrix generation
    // In production, use a proper Cauchy matrix
    let mut matrix = vec![vec![F::zero(); size]; size];
    for i in 0..size {
        for j in 0..size {
            matrix[i][j] = F::from((i + j + 1) as u64);
        }
    }
    matrix
}

/// Generate ARK constants for Poseidon
fn generate_ark_constants<F: PrimeField>(
    width: usize,
    full_rounds: usize,
    partial_rounds: usize,
) -> Vec<Vec<F>> {
    // Simplified constant generation
    // In production, use proper random generation
    let total_rounds = full_rounds + partial_rounds;
    let mut constants = Vec::with_capacity(total_rounds);

    for round in 0..total_rounds {
        let mut round_constants = Vec::with_capacity(width);
        for i in 0..width {
            round_constants.push(F::from(((round * width + i + 1) * 7919) as u64));
        }
        constants.push(round_constants);
    }

    constants
}

/// Compute Poseidon hash of field elements
pub fn poseidon_hash<F: PrimeField + Absorb>(inputs: &[F]) -> F {
    let params = poseidon_parameters::<F>();
    let mut sponge = PoseidonSponge::new(&params);

    for input in inputs {
        sponge.absorb(input);
    }

    let output = sponge.squeeze_field_elements(1);
    output[0]
}

/// Gadget for Poseidon hash in circuits
pub struct PoseidonHashGadget<F: PrimeField + Absorb> {
    #[allow(dead_code)]
    params: PoseidonConfig<F>,
}

impl<F: PrimeField + Absorb> PoseidonHashGadget<F> {
    pub fn new(params: PoseidonConfig<F>) -> Self {
        Self { params }
    }

    /// Hash field elements in circuit
    pub fn hash(
        &self,
        cs: ConstraintSystemRef<F>,
        inputs: &[FpVar<F>],
    ) -> Result<FpVar<F>, SynthesisError> {
        // This is a simplified implementation
        // In production, use ark-crypto-primitives gadgets

        // For now, compute hash outside and allocate as witness
        let concrete_inputs: Vec<F> = inputs
            .iter()
            .map(|v| v.value().unwrap_or(F::zero()))
            .collect();

        let hash_value = poseidon_hash(&concrete_inputs);

        // Allocate hash as witness
        let hash_var = FpVar::new_witness(cs, || Ok(hash_value))?;

        // Add constraints to verify the hash
        // In production, implement full Poseidon permutation constraints

        Ok(hash_var)
    }
}

/// Convert bytes to field elements for hashing
pub fn bytes_to_field_elements<F: PrimeField>(bytes: &[u8]) -> Vec<F> {
    // Pack bytes into field elements (31 bytes per field element for safety)
    const BYTES_PER_FIELD: usize = 31;
    let mut field_elements = Vec::new();

    for chunk in bytes.chunks(BYTES_PER_FIELD) {
        let mut padded = vec![0u8; 32];
        padded[..chunk.len()].copy_from_slice(chunk);
        let field_element = F::from_be_bytes_mod_order(&padded);
        field_elements.push(field_element);
    }

    field_elements
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381::Fr;
    use ark_ff::Zero;

    #[test]
    fn test_poseidon_hash() {
        let inputs = vec![Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)];
        let hash1 = poseidon_hash(&inputs);
        let hash2 = poseidon_hash(&inputs);

        // Hash should be deterministic
        assert_eq!(hash1, hash2);

        // Hash should not be zero
        assert_ne!(hash1, Fr::zero());
    }

    #[test]
    fn test_bytes_to_field_elements() {
        let bytes = b"Hello, Kolme!";
        let field_elements: Vec<Fr> = bytes_to_field_elements(bytes);

        assert_eq!(field_elements.len(), 1); // Short string fits in one field element
        assert_ne!(field_elements[0], Fr::zero());
    }

    #[test]
    fn test_different_inputs_different_hashes() {
        let inputs1 = vec![Fr::from(1u64), Fr::from(2u64)];
        let inputs2 = vec![Fr::from(1u64), Fr::from(3u64)];

        let hash1 = poseidon_hash(&inputs1);
        let hash2 = poseidon_hash(&inputs2);

        assert_ne!(hash1, hash2);
    }
}