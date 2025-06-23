use std::{ops::Deref, path::Path};

use crate::KolmeMerkleStore;
use anyhow::Context;
use fjall::{PartitionCreateOptions, PersistMode};

#[derive(Clone)]
pub enum FjallBlock {
    Handle(fjall::PartitionHandle),
    Owned(fjall::Keyspace, fjall::PartitionHandle),
}

impl Deref for FjallBlock {
    type Target = fjall::PartitionHandle;

    fn deref(&self) -> &Self::Target {
        match self {
            FjallBlock::Handle(ref part) => part,
            FjallBlock::Owned(_, ref part) => part,
        }
    }
}

impl FjallBlock {
    pub fn try_from_fjall_merkle_store<'a>(
        store: &KolmeMerkleStore,
        partition: impl Into<Option<&'a str>>,
    ) -> anyhow::Result<Self> {
        let fjall = store
            .fjall()
            .context("Provided `store` is not a KolmeMerkleStore::Fjall variant")?;

        let handle = if let Some(partition) = partition.into() {
            let keyspace = fjall.get_keyspace();

            keyspace.open_partition(partition.as_ref(), PartitionCreateOptions::default())?
        } else {
            fjall.handle.clone()
        };

        Ok(FjallBlock::Handle(handle))
    }

    pub fn try_from_options(partition: &str, path: &Path) -> anyhow::Result<Self> {
        let keyspace = fjall::Config::new(path).open().with_context(|| {
            format!(
                "Unable to open fjall keyspace with `fjall_dir` {}",
                path.display()
            )
        })?;
        let fjall_block = keyspace.open_partition(partition, PartitionCreateOptions::default())?;

        Ok(FjallBlock::Owned(keyspace, fjall_block))
    }

    pub fn persist(&self, mode: PersistMode) -> fjall::Result<()> {
        match self {
            FjallBlock::Owned(keyspace, _) => keyspace.persist(mode),
            _ => Ok(()),
        }
    }
}
