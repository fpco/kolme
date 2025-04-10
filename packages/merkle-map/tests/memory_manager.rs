use merkle_map::*;

async fn memory_manager_helper(size: u32) {
    let mut m = MerkleMap::new();
    for i in 0..size {
        m.insert(i, i * 2);
    }

    let mut store = MerkleMemoryStore::default();
    let manager = MerkleManager::default();
    let contents = manager.save(&mut store, &m).await.unwrap();

    let m2 = manager.load(&mut store, contents.hash).await.unwrap();

    assert_eq!(m, m2);
}

#[tokio::test]
async fn memory_manager_0() {
    memory_manager_helper(0).await
}

#[tokio::test]
async fn memory_manager_1() {
    memory_manager_helper(1).await
}

#[tokio::test]
async fn memory_manager_2() {
    memory_manager_helper(2).await
}

#[tokio::test]
async fn memory_manager_10() {
    memory_manager_helper(10).await
}

#[tokio::test]
async fn memory_manager_16() {
    memory_manager_helper(16).await
}

#[tokio::test]
async fn memory_manager_17() {
    memory_manager_helper(17).await
}

#[tokio::test]
async fn memory_manager_1000() {
    memory_manager_helper(1000).await
}
