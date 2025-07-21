use merkle_map::{MerkleManager, MerkleMap, MerkleMemoryStore};

#[tokio::test]
async fn force_eviction() {
    let mut m = MerkleMap::<u64, u64>::new();

    for i in 0..4096 {
        m.insert(i, i);
    }

    let manager = MerkleManager::new(1);
    let mut store = MerkleMemoryStore::default();

    let contents = manager.save(&mut store, &m).await.unwrap();

    let m2 = manager.load(&mut store, contents.hash).await.unwrap();

    assert_eq!(m, m2);
}
