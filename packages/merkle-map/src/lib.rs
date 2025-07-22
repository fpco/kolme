#[cfg(test)]
mod tests;

pub mod api;
mod impls;
mod memory_store;
mod merkle_deserializer;
mod merkle_serializer;
mod traits;
mod types;
mod vec;

pub use api::{load, save};
pub use impls::MerkleLockable;
pub use memory_store::*;
pub use merkle_deserializer::*;
pub use merkle_serializer::*;
pub use shared::types::Sha256Hash;
pub use traits::*;
pub use types::*;
pub use vec::MerkleVec;

// Internal prelude
use std::{borrow::Borrow, sync::Arc};
