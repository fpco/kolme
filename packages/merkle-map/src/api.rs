//! Helper types and functions for buffer reading and writing.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    sync::Arc,
};

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
    orig_hash: Sha256Hash,
) -> Result<Arc<MerkleContents>, MerkleSerialError> {
    // We want this code to be as parallelized as possible. That means
    // discovering as many needed layers as possible in each iteration.
    // This is a fairly complex algorithm, the basic idea is:
    //
    // * Keep a list of "layers we know need to be loaded"
    // * Load those all up
    // * If anything didn't come through, error out due to missing hashes
    // * Otherwise, process each new layer
    // * If it has children we haven't loaded yet, add those to the list to load and add a reverse dependency to fill in the layer when ready
    // * If all children have been loaded, then populate the contents
    let mut layers = HashMap::<Sha256Hash, MerkleLayerContents>::new();
    let mut all_contents = HashMap::<Sha256Hash, Arc<MerkleContents>>::new();
    let mut reverses = HashMap::<Sha256Hash, HashSet<Sha256Hash>>::new();
    let mut needed = vec![orig_hash];
    let mut missing = HashSet::<Sha256Hash>::new();
    let mut to_process = BTreeSet::<Sha256Hash>::new();

    while !needed.is_empty() {
        #[cfg(debug_assertions)]
        {
            for hash in &needed {
                assert!(!layers.contains_key(hash));
                assert!(!all_contents.contains_key(hash));
            }
        }
        store.load_by_hashes(&needed, &mut layers).await?;

        // Swap out the old needed list and process
        let old_needed = std::mem::take(&mut needed);

        for hash in old_needed {
            if layers.contains_key(&hash) {
                // OK, we got the layer we wanted, so add it to the to_process queue.
                // We'll iterate over everything we've loaded there, since we may
                // need to process parent layers too.
                to_process.insert(hash);
            } else {
                // The layer didn't get loaded, so report an error.
                // We could exit immediately, but this allows us to collect
                // a more meaningful error message.
                missing.insert(hash);
            };
        }

        // Error out if anything was missing
        if !missing.is_empty() {
            return Err(MerkleSerialError::HashesNotFound { hashes: missing });
        }

        // Now begin the to_process loop, handling all reverse dependencies in the process.
        while let Some(hash) = to_process.pop_first() {
            debug_assert!(
                !all_contents.contains_key(&hash),
                "to_process loop: all_contents already contains {hash}"
            );
            let layer = layers
                .get(&hash)
                .expect("Logic error: missing layer in to_process loop");

            // Check if all the children are fully loaded.
            let mut children = vec![];
            for child in &layer.children {
                match all_contents.get(child) {
                    // This child is fully loaded, add it
                    Some(child) => children.push(child.clone()),
                    // Not fully loaded yet
                    None => {
                        // If we have the layer, we're just waiting for children.
                        // If we don't have the layer, add it to needed.
                        if !layers.contains_key(child) {
                            needed.push(*child);
                        }

                        // And we want to be notified when this child is completed.
                        // Add it to the reverse dependencies.
                        reverses.entry(*child).or_default().insert(hash);
                    }
                }
            }

            // Did we get all the children?
            if children.len() != layer.children.len() {
                // We haven't fully loaded all children. We've already
                // registered necessary reverse entries above. Now we wait.
                continue;
            }

            // All children are fully loaded, so we can create our MerkleContents.
            let contents = Arc::new(MerkleContents {
                hash,
                payload: layer.payload.clone(),
                children: children.into(),
            });

            // If this is the original hash, we're done with our work!
            if hash == orig_hash {
                return Ok(contents);
            }

            // For all other hashes, we're the child of something else. In that case,
            // update all_contents with our contents...
            let old = all_contents.insert(hash, contents);
            debug_assert!(
                old.is_none(),
                "Logic error: hash {hash} already fully loaded"
            );

            // ... and then process all the reverse dependencies.
            let reverses = reverses
                .remove(&hash)
                .expect("Logic error: impossible missing reverses");

            // Process all reverses
            for reverse in reverses {
                // Only bother processing if we haven't already
                if !all_contents.contains_key(&reverse) {
                    to_process.insert(reverse);
                }
            }
        }
    }

    panic!("Logic error: impossible missing contents, we should have discovered the contents in the processing loop")
}
