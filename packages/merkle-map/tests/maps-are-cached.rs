use std::sync::atomic::AtomicU32;

use merkle_map::{
    load, save, MerkleDeserializeRaw, MerkleMap, MerkleMemoryStore, MerkleSerializeRaw,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Value(u64);

static DESERIALIZE_COUNT: AtomicU32 = AtomicU32::new(0);

impl MerkleSerializeRaw for Value {
    fn merkle_serialize_raw(
        &self,
        serializer: &mut merkle_map::MerkleSerializer,
    ) -> Result<(), merkle_map::MerkleSerialError> {
        serializer.store(&self.0)
    }
}

impl MerkleDeserializeRaw for Value {
    fn merkle_deserialize_raw(
        deserializer: &mut merkle_map::MerkleDeserializer,
    ) -> Result<Self, merkle_map::MerkleSerialError> {
        DESERIALIZE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        deserializer.load().map(Self)
    }
}

#[tokio::test]
async fn maps_are_cached() {
    let mut m = MerkleMap::new();
    m.insert(1u8, Value(1));
    let mut store = MerkleMemoryStore::default();
    let hash = save(&mut store, &m).await.unwrap();
    assert_eq!(
        DESERIALIZE_COUNT.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    let m2 = load(&mut store, hash).await.unwrap();
    assert_eq!(m, m2);
    assert_eq!(
        DESERIALIZE_COUNT.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    std::mem::drop(m);
    let m3 = load(&mut store, hash).await.unwrap();
    assert_eq!(m2, m3);
    assert_eq!(
        DESERIALIZE_COUNT.load(std::sync::atomic::Ordering::Relaxed),
        0
    );
    std::mem::drop(m2);
    std::mem::drop(m3);

    let m4: MerkleMap<u8, Value> = load(&mut store, hash).await.unwrap();
    let mut m5 = MerkleMap::new();
    m5.insert(1u8, Value(1));
    assert_eq!(m4, m5);
    assert_eq!(
        DESERIALIZE_COUNT.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}
