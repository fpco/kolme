//! Helper types and functions for buffer reading and writing.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

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

/// Load the value at the given hash
pub async fn load<T: MerkleDeserializeRaw, Store: MerkleStore>(
    store: &mut Store,
    hash: Sha256Hash,
) -> Result<T, MerkleSerialError> {
    let layer =
        store
            .load_by_hash(hash)
            .await?
            .ok_or_else(|| MerkleSerialError::HashesNotFound {
                hashes: HashSet::from_iter([hash]),
            })?;
    let mut deserializer = MerkleDeserializer::new(hash, layer.payload, store);
    let value = T::merkle_deserialize_raw(&mut deserializer)?;
    let contents = Arc::new(deserializer.finish()?);
    value.set_merkle_contents_raw(&contents);
    Ok(value)
}

pub(crate) async fn get_or_load_payload<Store: MerkleStore>(
    store: &mut Store,
    hash: Sha256Hash,
) -> Result<
    (
        MerkleLayerContents,
        HashMap<Sha256Hash, MerkleLayerContents>,
    ),
    MerkleSerialError,
> {
    let mut layers = HashMap::new();
    let mut to_process = vec![hash];

    loop {
        let Some(hash) = to_process.pop() else { break };

        let layer =
            store
                .load_by_hash(hash)
                .await?
                .ok_or_else(|| MerkleSerialError::HashesNotFound {
                    hashes: HashSet::from_iter([hash]),
                })?;

        to_process.extend_from_slice(&layer.children);
        layers.insert(hash, layer);
    }

    Ok((
        layers.get(&hash).expect("Impossible missing hash").clone(),
        layers,
    ))
}
