//! The goal of this test is to ensure that subtrees are not
//! reserialized or resaved in the data store. We have a top
//! level data structure that contains a MerkleMap, and the values
//! of that MerkleMap will produce a panic if serialized twice.
//!
//! Additionally, we have a debug assert within the in-memory store
//! to error out if we save the same hash twice.

use std::{collections::HashMap, sync::OnceLock};

use merkle_map::{
    api::{self, save_merkle_contents},
    save, MerkleDeserialize, MerkleLayerContents, MerkleMap, MerkleMemoryStore, MerkleSerialError,
    MerkleSerialize, MerkleStore, Sha256Hash,
};

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

struct SampleState {
    pub big_map: MerkleMap<u64, u64>,
}

impl Default for SampleState {
    fn default() -> Self {
        Self {
            big_map: (0..20).map(|i| (i, i)).collect(),
        }
    }
}

impl MerkleSerialize for SampleState {
    fn merkle_serialize(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), merkle_map::MerkleSerialError> {
        let Self { big_map } = self;
        serializer.store_by_hash(big_map)?;
        Ok(())
    }
}

impl MerkleDeserialize for SampleState {
    fn merkle_deserialize(
        deserializer: &mut merkle_map::MerkleDeserializer,
        _version: usize,
    ) -> Result<Self, MerkleSerialError> {
        let big_map = deserializer.load_by_hash()?;
        Ok(Self { big_map })
    }
}

#[tokio::test]
async fn save_externally_loaded_state() {
    let state = SampleState::default();
    let mut store = TracingMerkleMemoryStore::default();

    // with this we simulate state loaded into DB on another node
    // as a result checking its hash should return that we have it already
    let contents = api::serialize(&state).unwrap();
    let hash = contents.hash();
    save_merkle_contents(&mut store, contents).await.unwrap();

    // then we could load it back using merkle_map::api
    api::load::<SampleState, _>(&mut store, hash).await.unwrap();

    store.reset();

    // on a save the code is expected to save at least the top level hash and
    // load in PG store should use merkle_cache but that doesn't look
    // to be reproducible with MemoryStore
    // should pick it up from instead of going to the store
    api::save(&mut store, &state).await.unwrap();
    assert!(!store.hash_saves.is_empty());

    store.reset();

    // another interation should shouldn't change anything
    api::save(&mut store, &state).await.unwrap();
    assert!(!store.hash_saves.is_empty());
}

#[derive(Default)]
struct TracingMerkleMemoryStore {
    pub hash_saves: Vec<Sha256Hash>,
    memory_store: MerkleMemoryStore,
}

impl TracingMerkleMemoryStore {
    fn reset(&mut self) {
        self.hash_saves.clear();
    }
}

impl MerkleStore for TracingMerkleMemoryStore {
    async fn load_by_hashes(
        &mut self,
        hashes: &[Sha256Hash],
        dest: &mut HashMap<Sha256Hash, MerkleLayerContents>,
    ) -> Result<(), MerkleSerialError> {
        self.memory_store.load_by_hashes(hashes, dest).await
    }

    async fn save_by_hash(&mut self, layer: &MerkleLayerContents) -> Result<(), MerkleSerialError> {
        self.hash_saves.push(layer.payload.hash());
        self.memory_store.save_by_hash(layer).await
    }

    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        self.memory_store.contains_hash(hash).await
    }
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
