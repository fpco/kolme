use std::{env::VarError, ffi::OsStr, time::Duration};

use criterion::{measurement::Measurement, BenchmarkGroup};
use kolme::MerkleMap;
use regex::Regex;
use tokio::runtime::Handle;

use super::r#trait::{BenchmarkGroupExt, StoreEnv};

pub enum Filter {
    ByRegex(Regex),
    ExecuteAll,
    DoNotRun,
}

impl Filter {
    pub fn from_env(name: impl AsRef<OsStr>) -> Result<Filter, VarError> {
        let result = std::env::var(name);
        let result = result.as_deref();
        match result {
            Ok("false") => Ok(Filter::DoNotRun),
            Ok("true") | Err(VarError::NotPresent) => Ok(Filter::ExecuteAll),
            Ok(regex) => Ok(Filter::ByRegex(Regex::new(regex).expect("Invalid regex"))),
            Err(e) => Err(e.clone()),
        }
    }

    fn is_match(&self, name: &str) -> bool {
        match self {
            Filter::ByRegex(regex) => regex.is_match(name),
            Filter::ExecuteAll => true,
            Filter::DoNotRun => false,
        }
    }
}

#[derive(Default, Clone)]
pub struct RawMerkleMap(pub MerkleMap<Vec<u8>, Vec<u8>>, pub usize);

pub struct BenchmarkGroupRunner<'a, M>
where
    M: Measurement,
{
    handle: Handle,
    initial_filter_regex: Filter,
    reserialization_filter_regex: Filter,
    pub group: BenchmarkGroup<'a, M>,
}

impl<'a, M> BenchmarkGroupRunner<'a, M>
where
    M: Measurement<Value = Duration>,
{
    pub fn new(group: BenchmarkGroup<'a, M>, handle: Handle) -> Self {
        let initial_filter_regex = Filter::from_env("INITIAL_FILTER_REGEX")
            .expect("Unable to get filter from INITIAL_FILTER_REGEX");
        let reserialization_filter_regex = Filter::from_env("RESERIALIZATION_FILTER_REGEX")
            .expect("Unable to get filter from RESERIALIZATION_FILTER_REGEX");

        Self {
            initial_filter_regex,
            reserialization_filter_regex,
            handle,
            group,
        }
    }

    pub fn run_initial<Store: StoreEnv, MapFactory: Fn() -> RawMerkleMap>(
        &mut self,
        name: impl AsRef<str>,
        params: Store::Params,
        factory: MapFactory,
    ) {
        if self.initial_filter_regex.is_match(name.as_ref()) {
            self.group.bench_group_initial::<Store, MapFactory>(
                self.handle.clone(),
                name.as_ref(),
                params,
                factory,
            )
        }
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
        if self.reserialization_filter_regex.is_match(name.as_ref()) {
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
}
