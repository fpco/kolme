use kolme::MerkleManager;
use tokio::runtime::Handle;

use super::core::RawMerkleMap;

pub trait StoreEnv {
    type Params: Clone;

    async fn new(params: Self::Params) -> Self;
    async fn run(&mut self, manager: &MerkleManager, map: RawMerkleMap);
    async fn cleanup(&mut self);
}

pub trait BenchmarkGroupExt {
    fn bench_group_initial<Store, MapFactory>(
        &mut self,
        handle: Handle,
        name: &str,
        params: Store::Params,
        factory: MapFactory,
    ) where
        Store: StoreEnv,
        MapFactory: Fn() -> RawMerkleMap;

    fn bench_group_reserialization<Store, MapFactory, MapUpdater>(
        &mut self,
        handle: Handle,
        name: &str,
        params: Store::Params,
        factory: MapFactory,
        update: MapUpdater,
    ) where
        Store: StoreEnv,
        MapFactory: Fn() -> RawMerkleMap,
        MapUpdater: Fn(&mut RawMerkleMap);
}
