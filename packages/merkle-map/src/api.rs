//! Helper types and functions for buffer reading and writing.

use std::{collections::HashSet, sync::Arc};

use shared::types::Sha256Hash;
use smallvec::SmallVec;

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
    contents: &MerkleContents,
) -> Result<(), MerkleSerialError> {
    // First check if our hash already exists. If so, we assume
    // all children are also present.
    if store.contains_hash(contents.hash).await? {
        return Ok(());
    }

    // TODO avoid the recursive call, instead use a Vec of work

    // If the hash doesn't exist, write the children first.
    // This is to ensure that all child data is present before
    // writing the parent. This is what allows us to have the short-circuit
    // optimization above.
    let mut children = SmallVec::with_capacity(contents.children.len());
    for child in contents.children.iter() {
        children.push(child.hash);
        let future = Box::pin(save_merkle_contents(store, child));
        future.await?;
    }

    // And finally write the actual contents.
    let layer = MerkleLayerContents {
        payload: contents.payload.clone(),
        children,
    };
    store.save_by_hash(contents.hash, &layer).await?;

    Ok(())
}

/// Serialize a save a value.
pub async fn save<T: MerkleSerializeRaw, Store: MerkleStore>(
    store: &mut Store,
    value: &T,
) -> Result<Arc<MerkleContents>, MerkleSerialError> {
    let contents = serialize(value)?;
    save_merkle_contents(store, &contents).await?;
    Ok(contents)
}

/// Deserialize a value from the given payload.
///
/// This relies on all layers fitting in the LRU cache of the manager. This is finicky;
/// consider moving to other methods.
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
