//! The goal of this test is to ensure that subtrees are not
//! reserialized or resaved in the data store. We have a top
//! level data structure that contains a MerkleMap, and the values
//! of that MerkleMap will produce a panic if serialized twice.
//!
//! Additionally, we have a debug assert within the in-memory store
//! to error out if we save the same hash twice.

use std::sync::OnceLock;

use merkle_map::{save, MerkleMap, MerkleMemoryStore, MerkleSerialize};

struct MyAppState {
    big_map: MerkleMap<u64, OnlySerializeOnce>,
}

#[derive(Default, Clone)]
struct OnlySerializeOnce {
    was_serialized: OnceLock<()>,
}

impl Default for MyAppState {
    fn default() -> Self {
        MyAppState {
            big_map: std::iter::once((42, OnlySerializeOnce::default())).collect(),
        }
    }
}

impl MerkleSerialize for MyAppState {
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), merkle_map::MerkleSerialError> {
        let Self { big_map } = self;
        serializer.store_by_hash(big_map)?;
        Ok(())
    }
}

impl MerkleSerialize for OnlySerializeOnce {
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), merkle_map::MerkleSerialError> {
        self.was_serialized
            .set(())
            .expect("OnlySerializeOnce was serialized twice");
        serializer.store_byte(42);
        Ok(())
    }
}

#[tokio::test]
async fn no_reserialize_subtree() {
    let state = MyAppState::default();

    let mut store = MerkleMemoryStore::default();
    save(&mut store, &state).await.unwrap();
    save(&mut store, &state).await.unwrap();
}

// FIXME: should we ensure that store and store_by_hash have the
// same behavior for MerkleMaps?
// struct MyAppStateWrapper(MyAppState);

// #[tokio::test]
// async fn store_matches_store_by_hash() {
//     let mut store = MerkleMemoryStore::default().disallow_dupes();
//     let hash1 = save(&mut store, &MyAppState::default()).await.unwrap();
//     let hash2 = save(&mut store, &MyAppStateWrapper(MyAppState::default()))
//         .await
//         .unwrap();
//     assert_eq!(hash1, hash2);
// }

// impl MerkleSerialize for MyAppStateWrapper {
//     fn merkle_serialize(
//         &self,
//         serializer: &mut merkle_map::MerkleSerializer,
//     ) -> Result<(), merkle_map::MerkleSerialError> {
//         serializer.store(&self.0.big_map)?;
//         Ok(())
//     }
// }
