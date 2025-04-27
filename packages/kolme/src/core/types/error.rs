use crate::core::*;

#[derive(thiserror::Error, Debug)]
pub enum KolmeError {
    #[error("Invalid nonce provided for pubkey {pubkey}, account {account_id}. Expected: {expected}. Received: {actual}.")]
    InvalidNonce {
        pubkey: PublicKey,
        account_id: AccountId,
        expected: AccountNonce,
        actual: AccountNonce,
    },
}
