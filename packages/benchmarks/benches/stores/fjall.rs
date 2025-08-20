use super::{core::RawMerkleMap, r#trait::StoreEnv};
use fjall::PersistMode;

#[derive(Copy, Clone)]
pub struct StoreOptions {
    pub dirname: &'static str,
}

pub struct Store {
    store: kolme_store::fjall::merkle::MerkleFjallStore,
    _dirname: &'static str,
    _dir: tempdir::TempDir,
}

impl StoreEnv for Store {
    type Params = StoreOptions;

    async fn new(params: Self::Params) -> Self {
        let dir = tempdir::TempDir::new(params.dirname).unwrap();
        Store {
            store: kolme_store::fjall::merkle::MerkleFjallStore::new(&dir)
                .expect("Unable to construct fjall handle"),
            _dirname: params.dirname,
            _dir: dir,
        }
    }

    async fn run(&mut self, map: RawMerkleMap) {
        merkle_map::save(&mut self.store.clone(), &map.0)
            .await
            .expect("Unable to save MerkleMap contents");

        self.store
            .keyspace
            .persist(PersistMode::SyncAll)
            .expect("Unable to persist MerkleMap contents");
    }

    async fn cleanup(&mut self) {}
}
