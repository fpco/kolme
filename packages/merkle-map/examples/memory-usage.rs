use std::collections::{BTreeMap, HashMap};

use clap::Parser;
use merkle_map::MerkleMap;
use rand::{rngs::StdRng, Rng, SeedableRng};
use sysinfo::{Pid, System};

#[derive(clap::Parser)]
enum Cmd {
    Btreemap,
    Hashmap,
    Merklemap,
}

fn main() {
    match Cmd::parse() {
        Cmd::Btreemap => single::<BTreeMap<u64, u64>>(),
        Cmd::Hashmap => single::<HashMap<u64, u64>>(),
        Cmd::Merklemap => single::<MerkleMap<u64, u64>>(),
    }
}

fn single<T: Maplike<K = u64, V = u64>>() {
    let mut m = T::default();
    let mut rng = StdRng::seed_from_u64(0);

    for _ in 0..20000 {
        let k = rng.gen();
        let v = rng.gen();
        m.insert(k, v);
    }

    let pid = Pid::from(std::process::id() as usize);
    let mut system = System::new_all();
    system.refresh_all();
    let process = system.process(pid).unwrap();
    println!("{}", process.memory());
}

trait Maplike: Default {
    type K;
    type V;

    fn insert(&mut self, k: Self::K, v: Self::V);
}

impl Maplike for BTreeMap<u64, u64> {
    type K = u64;
    type V = u64;

    fn insert(&mut self, k: Self::K, v: Self::V) {
        self.insert(k, v);
    }
}

impl Maplike for HashMap<u64, u64> {
    type K = u64;
    type V = u64;

    fn insert(&mut self, k: Self::K, v: Self::V) {
        self.insert(k, v);
    }
}

impl Maplike for MerkleMap<u64, u64> {
    type K = u64;
    type V = u64;

    fn insert(&mut self, k: Self::K, v: Self::V) {
        self.insert(k, v);
    }
}
