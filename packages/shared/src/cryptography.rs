#[cfg(feature = "realcryptography")]
mod real;

#[cfg(all(not(feature = "realcryptography"), feature = "cosmwasm"))]
mod cosmwasm;

#[cfg(feature = "realcryptography")]
pub use real::*;

#[cfg(all(not(feature = "realcryptography"), feature = "cosmwasm"))]
pub use cosmwasm::*;

#[derive(Debug, thiserror::Error)]
pub enum CompressPublicKeyError {
    #[error("Wrong key length, expected {expected}, actual {actual}")]
    WrongUncompressedLen { expected: u32, actual: u32 },
    #[error("Wrong starting bytes, expected {expected}, actual {actual}")]
    WrongStartingByte { expected: u8, actual: u8 },
}

/// Compress an ecdsa public key.
///
/// This converts it from the uncompressed 65 byte representation to the
/// compressed 33 byte representation.
pub fn compress_public_key(uncompressed: &[u8]) -> Result<[u8; 33], CompressPublicKeyError> {
    if uncompressed.len() != 65 {
        return Err(CompressPublicKeyError::WrongUncompressedLen {
            expected: 65,
            actual: uncompressed.len() as u32,
        });
    }
    if uncompressed[0] != 0x04 {
        return Err(CompressPublicKeyError::WrongStartingByte {
            expected: 0x04,
            actual: uncompressed[0],
        });
    }

    let is_even = (uncompressed[64] & 1) == 0;

    let x = &uncompressed[1..=32];

    let mut res = [0; 33];
    res[0] = if is_even { 0x02 } else { 0x03 };
    res[1..].copy_from_slice(x);

    Ok(res)
}

#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct SignatureWithRecovery {
    pub recid: RecoveryId,
    pub sig: Signature,
}
