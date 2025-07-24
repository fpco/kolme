//! Helper types and functions for buffer reading and writing.

use std::{collections::HashSet, sync::Arc};

use shared::types::Sha256Hash;

use crate::*;

impl MerkleSerialError {
    pub fn custom<E: std::error::Error + Send + Sync + 'static>(e: E) -> Self {
        Self::Custom(Box::new(e))
    }
}

impl std::fmt::Debug for MerkleContents {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("MerkleContents").field(&self.hash).finish()
    }
}

/// Serialize a value into a [MerkleContents] for later storage.
pub fn serialize<T: MerkleSerializeRaw + ?Sized>(
    value: &T,
) -> Result<Arc<MerkleContents>, MerkleSerialError> {
    if let Some(contents) = value.get_merkle_contents_raw() {
        return Ok(contents);
    }

    let mut serializer = MerkleSerializer::new();
    value.merkle_serialize_raw(&mut serializer)?;
    let contents = Arc::new(serializer.finish());
    value.set_merkle_contents_raw(&contents);
    Ok(contents)
}

/// Save a [MerkleContents] to the given store.
pub async fn save_merkle_contents<Store: MerkleStore>(
    store: &mut Store,
    contents: Arc<MerkleContents>,
) -> Result<(), MerkleSerialError> {
    // Bypass everything below: if this already exists, just exit.
    if store.contains_hash(contents.hash).await? {
        return Ok(());
    }

    // Work items consist of both the contents to write, and whether we need to
    // write children first. The workflow here is:
    //
    // 1. Add a contents and say we need to check children
    // 2. Pop item from the queue, if we don't need to check children, store immediately
    // 3. If we do need to check children, put this back on the queue with "no check children"
    //    and then add all children to the queue, saying they need to be checked.
    //
    // Motivation: we want to ensure we always write children layers before the parent.
    struct Work {
        contents: Arc<MerkleContents>,
        check_children: bool,
    }

    let mut work = vec![Work {
        contents,
        check_children: true,
    }];

    while let Some(Work {
        contents,
        check_children,
    }) = work.pop()
    {
        if check_children {
            let children = contents.children.clone();
            work.push(Work {
                contents,
                check_children: false,
            });
            for child in children.iter() {
                if !store.contains_hash(child.hash).await? {
                    work.push(Work {
                        contents: child.clone(),
                        check_children: true,
                    });
                }
            }
        } else {
            store
                .save_by_hash(
                    contents.hash,
                    &MerkleLayerContents {
                        payload: contents.payload.clone(),
                        children: contents.children.iter().map(|c| c.hash).collect(),
                    },
                )
                .await?;
        }
    }

    Ok(())
}

/// Serialize a save a value.
pub async fn save<T: MerkleSerializeRaw, Store: MerkleStore>(
    store: &mut Store,
    value: &T,
) -> Result<Arc<MerkleContents>, MerkleSerialError> {
    let contents = serialize(value)?;
    save_merkle_contents(store, contents.clone()).await?;
    Ok(contents)
}

/// Deserialize a value from the given [MerkleContents].
pub fn deserialize<T: MerkleDeserializeRaw>(
    contents: Arc<MerkleContents>,
) -> Result<T, MerkleSerialError> {
    let mut deserializer = MerkleDeserializer::new(contents.clone());
    let value = T::merkle_deserialize_raw(&mut deserializer)?;
    let contents = Arc::new(deserializer.finish()?);
    value.set_merkle_contents_raw(&contents);
    Ok(value)
}

/// Load the value at the given hash
pub async fn load<T: MerkleDeserializeRaw, Store: MerkleStore>(
    store: &mut Store,
    hash: Sha256Hash,
) -> Result<T, MerkleSerialError> {
    deserialize(load_merkle_contents(store, hash).await?)
}

/// Load up a [MerkleContents] from the store.
pub async fn load_merkle_contents<Store: MerkleStore>(
    store: &mut Store,
    hash: Sha256Hash,
) -> Result<Arc<MerkleContents>, MerkleSerialError> {
    let layer =
        store
            .load_by_hash(hash)
            .await?
            .ok_or_else(|| MerkleSerialError::HashesNotFound {
                hashes: std::iter::once(hash).collect(),
            })?;

    let mut children = vec![];
    let mut missing = HashSet::new();

    for child in layer.children {
        match Box::pin(load_merkle_contents(store, child)).await {
            Ok(child) => children.push(child),
            Err(MerkleSerialError::HashesNotFound { hashes }) => missing.extend(hashes.into_iter()),
            Err(e) => return Err(e),
        }
    }

    if !missing.is_empty() {
        return Err(MerkleSerialError::HashesNotFound { hashes: missing });
    }

    Ok(Arc::new(MerkleContents {
        hash,
        payload: layer.payload,
        children: children.into(),
    }))
}
