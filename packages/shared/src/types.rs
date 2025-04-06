#[cfg(feature = "sqlx")]
mod sqlx_impls;

use std::{fmt::Display, num::TryFromIntError};

/// Monotonically increasing identifier for actions sent to a bridge contract.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct BridgeActionId(pub u64);
impl BridgeActionId {
    pub fn start() -> Self {
        BridgeActionId(0)
    }

    pub fn next(self) -> BridgeActionId {
        BridgeActionId(self.0 + 1)
    }

    pub fn increment(&mut self) {
        self.0 += 1;
    }
}

impl Display for BridgeActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Monotonically increasing identifier for events coming from a bridge contract.
#[derive(
    serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Copy, Hash, Debug,
)]
pub struct BridgeEventId(pub u64);
impl BridgeEventId {
    pub fn start() -> BridgeEventId {
        BridgeEventId(0)
    }

    pub fn try_from_i64(id: i64) -> Result<Self, TryFromIntError> {
        id.try_into().map(BridgeEventId)
    }

    pub fn next(self) -> BridgeEventId {
        BridgeEventId(self.0 + 1)
    }

    pub fn increment(&mut self) {
        self.0 += 1;
    }

    pub fn prev(self) -> Option<BridgeEventId> {
        self.0.checked_sub(1).map(BridgeEventId)
    }
}

impl Display for BridgeEventId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "cosmwasm")]
mod cw_impls {
    use super::*;
    use cw_storage_plus::{KeyDeserialize, PrimaryKey};

    impl KeyDeserialize for BridgeEventId {
        type Output = Self;

        const KEY_ELEMS: u16 = 1;

        fn from_vec(value: Vec<u8>) -> cosmwasm_std::StdResult<Self::Output> {
            <u64 as KeyDeserialize>::from_vec(value).map(Self)
        }
    }

    impl PrimaryKey<'_> for BridgeEventId {
        type Prefix = ();
        type SubPrefix = ();
        type Suffix = Self;
        type SuperSuffix = Self;

        fn key(&self) -> Vec<cw_storage_plus::Key> {
            self.0.key()
        }
    }
}
