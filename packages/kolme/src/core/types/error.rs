use crate::core::*;

#[derive(thiserror::Error, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum KolmeError {
    #[error("Invalid nonce provided for pubkey {pubkey}, account {account_id}. Expected: {expected}. Received: {actual}.")]
    InvalidNonce {
        pubkey: Box<PublicKey>,
        account_id: AccountId,
        expected: AccountNonce,
        actual: AccountNonce,
    },
    #[error("This operation requires a validator to perform it, but {key} is not part of the validator set.")]
    NotAValidator { key: PublicKey },
    #[error("{0}")]
    Other(String),
}
