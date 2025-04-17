use crate::*;

#[derive(thiserror::Error, Debug)]
pub enum KolmeError {
    #[error("Invalid proposed block height ({proposed}) when adding block. Expected next height: {expected}.")]
    InvalidAddBlockHeight {
        proposed: BlockHeight,
        expected: BlockHeight,
    },
}
