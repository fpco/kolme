//! Key management for ZKP system

use ark_bls12_381::Bls12_381;
use ark_groth16::{ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use std::fs::{File, create_dir_all};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use crate::errors::{Result, ZkpAuthError};

/// Default directory for ZKP parameters
pub const DEFAULT_PARAMS_DIR: &str = "./data/zkp-params";

/// Key manager for loading and storing proving/verifying keys
pub struct KeyManager {
    params_dir: PathBuf,
}

impl KeyManager {
    /// Create a new key manager with default directory
    pub fn new() -> Self {
        Self {
            params_dir: PathBuf::from(DEFAULT_PARAMS_DIR),
        }
    }

    /// Create a key manager with custom directory
    pub fn with_dir<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            params_dir: dir.as_ref().to_path_buf(),
        }
    }

    /// Ensure the params directory exists
    fn ensure_dir(&self) -> Result<()> {
        create_dir_all(&self.params_dir)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;
        Ok(())
    }

    /// Load proving key from file
    pub fn load_proving_key(&self) -> Result<ProvingKey<Bls12_381>> {
        let path = self.params_dir.join("proving_key.bin");

        let mut file = File::open(&path)
            .map_err(|e| ZkpAuthError::KeyNotFound {
                key_type: "proving".to_string(),
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;

        ProvingKey::<Bls12_381>::deserialize_compressed(&buffer[..])
            .map_err(|e| ZkpAuthError::SerializationError { reason: e.to_string() })
    }

    /// Load verifying key from file
    pub fn load_verifying_key(&self) -> Result<VerifyingKey<Bls12_381>> {
        let path = self.params_dir.join("verifying_key.bin");

        let mut file = File::open(&path)
            .map_err(|e| ZkpAuthError::KeyNotFound {
                key_type: "verifying".to_string(),
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;

        VerifyingKey::<Bls12_381>::deserialize_compressed(&buffer[..])
            .map_err(|e| ZkpAuthError::SerializationError { reason: e.to_string() })
    }

    /// Save proving key to file
    pub fn save_proving_key(&self, pk: &ProvingKey<Bls12_381>) -> Result<()> {
        self.ensure_dir()?;
        let path = self.params_dir.join("proving_key.bin");

        let mut file = File::create(&path)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;

        let mut buffer = Vec::new();
        pk.serialize_compressed(&mut buffer)
            .map_err(|e| ZkpAuthError::SerializationError { reason: e.to_string() })?;

        file.write_all(&buffer)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;

        Ok(())
    }

    /// Save verifying key to file
    pub fn save_verifying_key(&self, vk: &VerifyingKey<Bls12_381>) -> Result<()> {
        self.ensure_dir()?;
        let path = self.params_dir.join("verifying_key.bin");

        let mut file = File::create(&path)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;

        let mut buffer = Vec::new();
        vk.serialize_compressed(&mut buffer)
            .map_err(|e| ZkpAuthError::SerializationError { reason: e.to_string() })?;

        file.write_all(&buffer)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;

        Ok(())
    }

    /// Check if keys exist
    pub fn keys_exist(&self) -> bool {
        let pk_path = self.params_dir.join("proving_key.bin");
        let vk_path = self.params_dir.join("verifying_key.bin");

        pk_path.exists() && vk_path.exists()
    }

    /// Initialize test keys for development/testing
    /// This creates minimal keys that work for testing but should NOT be used in production
    pub fn initialize_test_keys(&self) -> Result<()> {
        use crate::circuits::SocialIdentityCircuit;
        use crate::SocialPlatform;
        use ark_groth16::Groth16;
        use ark_snark::SNARK;
        use ark_bls12_381::Fr;
        use ark_std::rand::thread_rng;

        // Create a simple test circuit
        let circuit = SocialIdentityCircuit::<Fr>::new(
            Some("test_user".to_string()),
            Some(vec![1u8; 32]),
            Some(Fr::from(12345u64)),
            SocialPlatform::Twitter,
            Some(Fr::from(11111u64)),
        );

        let mut rng = thread_rng();

        // Generate test keys (this is NOT secure for production)
        let (pk, vk) = Groth16::<Bls12_381>::circuit_specific_setup(circuit, &mut rng)
            .map_err(|_| ZkpAuthError::TrustedSetupNotInitialized)?;

        // Save the keys
        self.save_proving_key(&pk)?;
        self.save_verifying_key(&vk)?;

        Ok(())
    }

    /// Load parameter hash for verification
    pub fn load_params_hash(&self) -> Result<String> {
        let path = self.params_dir.join("parameters.hash");

        let mut file = File::open(&path)
            .map_err(|e| ZkpAuthError::KeyNotFound {
                key_type: "hash".to_string(),
                path: path.display().to_string(),
                reason: e.to_string(),
            })?;

        let mut hash = String::new();
        file.read_to_string(&mut hash)
            .map_err(|e| ZkpAuthError::IOError { reason: e.to_string() })?;

        Ok(hash.trim().to_string())
    }
}

lazy_static::lazy_static! {
    /// Global key manager instance
    static ref KEY_MANAGER: KeyManager = KeyManager::new();
}

/// Get the global key manager
pub fn key_manager() -> &'static KeyManager {
    &KEY_MANAGER
}

/// Initialize key manager with custom directory
pub fn init_key_manager<P: AsRef<Path>>(dir: P) -> Result<()> {
    // This would need a more sophisticated approach for true initialization
    // For now, we'll just check if the directory is accessible
    let manager = KeyManager::with_dir(dir);
    manager.ensure_dir()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_key_manager_creation() {
        let manager = KeyManager::new();
        assert!(!manager.keys_exist());
    }

    #[test]
    fn test_key_manager_with_custom_dir() {
        let temp_dir = tempdir().unwrap();
        let manager = KeyManager::with_dir(temp_dir.path());
        assert!(!manager.keys_exist());
    }

    #[test]
    fn test_ensure_dir() {
        let temp_dir = tempdir().unwrap();
        let manager = KeyManager::with_dir(temp_dir.path().join("nested/path"));
        assert!(manager.ensure_dir().is_ok());
        assert!(temp_dir.path().join("nested/path").exists());
    }
}