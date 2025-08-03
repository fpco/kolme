use merkle_map::{load, save, MerkleContents, MerkleMap, MerkleMemoryStore};

#[tokio::test]
async fn deep() {
    let mut m = MerkleMap::new();
    for i in 1..100 {
        m.insert(i * 100_000u32, i);
    }
    let mut store = MerkleMemoryStore::default();
    let contents = save(&mut store, &m).await.unwrap();
    let max_depth = get_max_depth(&contents, 0);
    assert!(
        max_depth >= 4,
        "Max depth is {max_depth}, expecting at least 4"
    );
    let m2 = load(&mut store, contents.hash).await.unwrap();
    assert_eq!(m, m2);
}

fn get_max_depth(contents: &MerkleContents, depth: usize) -> usize {
    let mut max = depth + 1;
    for child in contents.children.iter() {
        max = max.max(get_max_depth(child, depth + 1));
    }
    max
}
