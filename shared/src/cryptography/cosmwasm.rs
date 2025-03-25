use std::fmt::Display;

use cosmwasm_std::HexBinary;

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub struct PublicKey(HexBinary);

impl Display for PublicKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0.to_hex())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct RecoveryId(u8);

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub struct Signature(HexBinary);
