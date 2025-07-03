use std::time::Duration;

use criterion::{BenchmarkGroup, measurement::Measurement};
use kolme::MerkleMap;
use tokio::runtime::Handle;

use super::r#trait::{BenchmarkGroupExt, StoreEnv};

#[derive(Default, Clone)]
pub struct RawMerkleMap(pub MerkleMap<Vec<u8>, Vec<u8>>);

pub struct BenchmarkGroupRunner<'a, M>
where
    M: Measurement,
{
    handle: Handle,
    group: BenchmarkGroup<'a, M>,
}

impl<'a, M> BenchmarkGroupRunner<'a, M>
where
    M: Measurement<Value = Duration>,
{
    pub fn new(group: BenchmarkGroup<'a, M>, handle: Handle) -> Self {
        Self { handle, group }
    }

    pub fn run_initial<Store: StoreEnv, MapFactory: Fn() -> RawMerkleMap>(
        &mut self,
        name: impl AsRef<str>,
        params: Store::Params,
        factory: MapFactory,
    ) {
        self.group.bench_group_initial::<Store, MapFactory>(
            self.handle.clone(),
            name.as_ref(),
            params,
            factory,
        )
    }

    pub fn run_reserialization<
        Store: StoreEnv,
        MapFactory: Fn() -> RawMerkleMap,
        MapUpdater: Fn(&mut RawMerkleMap),
    >(
        &mut self,
        name: impl AsRef<str>,
        params: Store::Params,
        factory: MapFactory,
        updater: MapUpdater,
    ) {
        self.group
            .bench_group_reserialization::<Store, MapFactory, MapUpdater>(
                self.handle.clone(),
                name.as_ref(),
                params,
                factory,
                updater,
            )
    }
}
