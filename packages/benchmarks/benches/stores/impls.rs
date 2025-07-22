use std::time::{Duration, Instant};

use super::{
    core::RawMerkleMap,
    r#trait::{BenchmarkGroupExt, StoreEnv},
};
use criterion::{measurement::Measurement, BenchmarkGroup};
use tokio::runtime::Handle;

impl<M> BenchmarkGroupExt for BenchmarkGroup<'_, M>
where
    M: Measurement<Value = Duration>,
{
    fn bench_group_initial<Store, MapFactory>(
        &mut self,
        handle: Handle,
        name: &str,
        params: Store::Params,
        factory: MapFactory,
    ) where
        Store: StoreEnv,
        MapFactory: Fn() -> RawMerkleMap,
    {
        self.bench_function(name, |b| {
            let factory = &factory;
            b.to_async(&handle).iter_custom(|iters| {
                let factory = &factory;
                let params = params.clone();
                async move {
                    let mut time = Duration::default();

                    for _ in 0..iters {
                        let mut store = Store::new(params.clone()).await;
                        let merkle_map = factory();

                        let run_time = Instant::now();
                        store.run(merkle_map).await;
                        time += run_time.elapsed();

                        store.cleanup().await;
                    }

                    time
                }
            })
        });
    }

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
        MapUpdater: Fn(&mut RawMerkleMap),
    {
        self.bench_function(name, |b| {
            let factory = &factory;
            let update = &update;
            b.to_async(&handle).iter_custom(|iters| {
                let factory = &factory;
                let update = &update;
                let params = params.clone();

                async move {
                    let mut time = Duration::default();

                    for _ in 0..iters {
                        let mut store = Store::new(params.clone()).await;
                        let mut merkle_map = factory();
                        store.run(merkle_map.clone()).await;
                        update(&mut merkle_map);

                        let run_time = Instant::now();
                        store.run(merkle_map).await;
                        time += run_time.elapsed();

                        store.cleanup().await;
                    }

                    time
                }
            })
        });
    }
}
