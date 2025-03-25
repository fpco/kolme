use cosmwasm_std::{Api, HexBinary, RecoverPubkeyError};
use shared::cosmos::*;
use shared::cryptography::{compress_public_key, CompressPublicKeyError};

#[derive(thiserror::Error, Debug)]
pub enum SignatureError {
    #[error(transparent)]
    Compress {
        #[from]
        source: CompressPublicKeyError,
    },
    #[error("Invalid signature {sig} with recovery_id {recid}: {source}.")]
    InvalidSignature {
        source: RecoverPubkeyError,
        sig: HexBinary,
        recid: u8,
    },
}

/// Validates the signature and returns the public key of the signer.
///
/// Returns the compressed signature.
pub(super) fn validate_signature(
    api: &dyn Api,
    hash: &[u8],
    SignatureWithRecovery { recid, sig }: &SignatureWithRecovery,
) -> Result<Vec<u8>, SignatureError> {
    let uncompressed = api
        .secp256k1_recover_pubkey(hash, sig, *recid)
        .map_err(|source| SignatureError::InvalidSignature {
            source,
            sig: sig.clone(),
            recid: *recid,
        })?;
    compress_public_key(&uncompressed).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{testing::MockApi, HexBinary};
    use k256::ecdsa::SigningKey;
    use rand::Rng;
    use sha2::{Digest, Sha256};

    #[test]
    fn test_signature() {
        const MSG: &str = "This is a test message";
        const SIGNATURE: &str = "D236913E08C9DE2BF776321FB8B32ABE1E6E92685E5D3BF9511F4F14FBC5C96C7A25427CA4150554AC4430C18DBD268DE9BB66E43892B9D37867970A9886B64F";
        const RECOVERY: u8 = 0;
        const PUBLIC: &str = "0264eb26609d15e709227b9ddc46c11a738b210bb237949aa86d7d490a35ae0f0a";
        // Secret key
        // 658c3528422eb527b4c108b8f6d1e5f629543c304ea49cf608c67794424291c4

        let message_hash = Sha256::digest(MSG);

        let pubkey = validate_signature(
            &MockApi::default(),
            &message_hash,
            &SignatureWithRecovery {
                recid: RECOVERY,
                sig: HexBinary::from_hex(SIGNATURE).unwrap(),
            },
        )
        .unwrap();

        assert_eq!(&pubkey, &hex::decode(PUBLIC).unwrap());
    }

    #[test]
    fn random_signing() {
        let mut rng = rand::thread_rng();
        let mut payload = vec![];
        for _ in 0..=rng.gen_range(20..=100) {
            payload.push(rand::random::<u8>());
        }

        let secret = k256::SecretKey::random(&mut rng);

        let (signature, recovery) = SigningKey::from(&secret)
            .sign_recoverable(&payload)
            .unwrap();

        let hash = Sha256::digest(&payload);

        let pubkey = validate_signature(
            &MockApi::default(),
            &hash,
            &SignatureWithRecovery {
                recid: recovery.to_byte(),
                sig: HexBinary::from(signature.to_vec()),
            },
        )
        .unwrap();

        assert_eq!(&*secret.public_key().to_sec1_bytes(), &pubkey);
    }
}
