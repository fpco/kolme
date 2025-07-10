mod stores {
    pub mod core;
    pub mod fjall;
    pub mod impls;
    pub mod postgres;
    pub mod r#trait;
}

use criterion::{criterion_group, criterion_main, Criterion};
use kolme::MerkleMap;
use rand::{Rng as _, RngCore as _};
use stores::core::{BenchmarkGroupRunner, RawMerkleMap};

fn generate_merkle_map(map_size: usize, content_size: usize) -> RawMerkleMap {
    let mut rng = rand::thread_rng();
    let mut merkle_map = MerkleMap::default();

    for _ in 0..map_size {
        let mut key = vec![0; content_size];
        let mut value = vec![0; content_size];
        rng.fill_bytes(&mut key);
        rng.fill_bytes(&mut value);
        merkle_map.insert(key, value);
    }

    RawMerkleMap(merkle_map, map_size)
}

fn update_merkle_map(probability_to_update: f64, merkle_map: &mut RawMerkleMap) {
    let keys: Vec<Vec<u8>> = merkle_map.0.keys().cloned().collect();
    let mut rng = rand::thread_rng();

    for key in keys {
        if rng.gen_bool(probability_to_update) {
            let mut value = vec![0; key.len()];
            rng.fill_bytes(&mut value);

            merkle_map.0.insert(key, value);
        }
    }
}

const MAP_SIZES: [usize; 5] = [4, 16, 256, 4096, 65535];
const CONTENT_SIZES: [usize; 5] = [64, 256, 1024, 10240, 20480];
const UPDATE_PERCENTS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

fn reserialization_benchmarks(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("Unable to construct Tokio runtime");
    let group = c.benchmark_group("Reserialization");
    let mut runner = BenchmarkGroupRunner::new(group, runtime.handle().clone());
    let postgres_options = stores::postgres::StoreOptions {
        url: std::env::var("DATABASE_URL").expect("DATABASE_URL env variable missing"),
    };
    let fjall_options = stores::fjall::StoreOptions {
        dirname: "fjall-dir",
    };
    let sizes = MAP_SIZES
        .into_iter()
        .flat_map(|map_size| {
            CONTENT_SIZES
                .into_iter()
                .map(move |content_size| (map_size.clone(), content_size))
        })
        .flat_map(|(msize, csize)| {
            UPDATE_PERCENTS
                .into_iter()
                .map(move |update| (update, msize.clone(), csize.clone()))
        });

    for (update_percent, map_size, content_size) in sizes {
        runner.run_reserialization::<stores::fjall::Store, _, _>(
            format!(
                "Fjall ({}%) [{}] [{}]",
                update_percent * 100.0,
                map_size,
                content_size,
            ),
            fjall_options.clone(),
            || generate_merkle_map(map_size.clone(), content_size.clone()),
            |map| update_merkle_map(0.2, map),
        );
        runner.run_reserialization::<stores::postgres::Store, _, _>(
            format!(
                "Postgres ({}%) [{}] [{}]",
                update_percent * 100.,
                map_size,
                content_size,
            ),
            postgres_options.clone(),
            || generate_merkle_map(map_size.clone(), content_size.clone()),
            |map| update_merkle_map(update_percent, map),
        );
    }
}

fn initial_insertion_benchmarks(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("Unable to construct Tokio runtime");
    let group = c.benchmark_group("Initial insertion");
    let mut runner = BenchmarkGroupRunner::new(group, runtime.handle().clone());
    let postgres_options = stores::postgres::StoreOptions {
        url: std::env::var("DATABASE_URL").expect("DATABASE_URL env variable missing"),
    };
    let fjall_options = stores::fjall::StoreOptions {
        dirname: "fjall-dir",
    };
    let sizes = MAP_SIZES.into_iter().flat_map(|map_size| {
        CONTENT_SIZES
            .into_iter()
            .map(move |content_size| (map_size.clone(), content_size))
    });

    for (map_size, content_size) in sizes {
        runner.run_initial::<stores::fjall::Store, _>(
            format!("Fjall [{}] [{}]", map_size, content_size),
            fjall_options.clone(),
            || generate_merkle_map(map_size.clone(), content_size.clone()),
        );
        runner.run_initial::<stores::postgres::Store, _>(
            format!("Postgres [{}] [{}]", map_size, content_size),
            postgres_options.clone(),
            || generate_merkle_map(map_size.clone(), content_size.clone()),
        );
    }

    runner.group.finish();
}

criterion_group!(
    name = insertions;
    config = Criterion::default().sample_size(500);
    targets =
        initial_insertion_benchmarks,
        reserialization_benchmarks
);
criterion_main!(insertions);
