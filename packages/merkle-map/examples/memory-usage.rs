use std::collections::{BTreeMap, HashMap};

use clap::Parser;
use merkle_map::MerkleMap;
use rand::{distributions::Alphanumeric, rngs::StdRng, Rng, SeedableRng};
use sysinfo::{Pid, System};

#[derive(clap::Parser)]
enum Cmd {
    Btreemap,
    Hashmap,
    Merklemap,
}

fn main() {
    match Cmd::parse() {
        Cmd::Btreemap => single::<BTreeMap<String, u64>>(),
        Cmd::Hashmap => single::<HashMap<String, u64>>(),
        Cmd::Merklemap => single::<MerkleMap<String, u64>>(),
    }
}

fn single<T: Maplike<K = String, V = u64>>() {
    let mut m = T::default();
    let mut rng = StdRng::seed_from_u64(0);

    for _ in 0..80000 {
        let k = (&mut rng).sample_iter(&Alphanumeric).take(8).map(char::from).collect();
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

impl Maplike for BTreeMap<String, u64> {
    type K = String;
    type V = u64;

    fn insert(&mut self, k: Self::K, v: Self::V) {
        self.insert(k, v);
    }
}

impl Maplike for HashMap<String, u64> {
    type K = String;
    type V = u64;

    fn insert(&mut self, k: Self::K, v: Self::V) {
        self.insert(k, v);
    }
}

impl Maplike for MerkleMap<String, u64> {
    type K = String;
    type V = u64;

    fn insert(&mut self, k: Self::K, v: Self::V) {
        self.insert(k, v);
    }
}
