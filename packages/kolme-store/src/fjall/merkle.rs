use std::{collections::HashMap, path::Path, sync::Arc};

use fjall::PartitionCreateOptions;
use merkle_map::{MerkleLayerContents, MerkleSerialError, MerkleStore, Sha256Hash};
use smallvec::SmallVec;

#[derive(Clone)]
pub struct MerkleFjallStore {
    pub keyspace: fjall::Keyspace,
    pub handle: fjall::PartitionHandle,
}

impl MerkleFjallStore {
    pub fn new(fjall_dir: impl AsRef<Path>) -> Result<Self, MerkleSerialError> {
        let keyspace = fjall::Config::new(fjall_dir)
            .open()
            .map_err(MerkleSerialError::custom)?;
        let handle = keyspace
            .open_partition("merkle", PartitionCreateOptions::default())
            .map_err(MerkleSerialError::custom)?;
        Ok(Self { keyspace, handle })
    }
}

struct Keys {
    payload: [u8; 33],
    children: [u8; 33],
}

impl Keys {
    fn from_hash(hash: Sha256Hash) -> Self {
        let mut payload = [0u8; 33];
        payload[..32].copy_from_slice(hash.as_array());
        payload[32] = b'p'; // payload
        let mut children = payload;
        children[32] = b'c';
        Keys { payload, children }
    }
}

impl MerkleFjallStore {
    pub(crate) fn load_by_hash(
        &mut self,
        hash: Sha256Hash,
    ) -> Result<Option<MerkleLayerContents>, MerkleSerialError> {
        let Keys { payload, children } = Keys::from_hash(hash);
        let Some(payload) = self
            .handle
            .get(payload)
            .map(|oslice| oslice.map(|slice| Arc::<[u8]>::from(slice.to_vec())))
            .map_err(MerkleSerialError::custom)?
        else {
            return Ok(None);
        };
        let Some(children) = self
            .handle
            .get(children)
            .map(|oslice| oslice.map(|slice| Arc::<[u8]>::from(slice.to_vec())))
            .map_err(MerkleSerialError::custom)?
        else {
            return Ok(None);
        };
        let children = parse_children(&children)?;
        Ok(Some(MerkleLayerContents {
            payload: merkle_map::CachedBytes::new_hash(hash, payload),
            children,
        }))
    }
}

impl MerkleStore for MerkleFjallStore {
    async fn load_by_hashes(
        &mut self,
        hashes: &[Sha256Hash],
        dest: &mut HashMap<Sha256Hash, MerkleLayerContents>,
    ) -> Result<(), MerkleSerialError> {
        for hash in hashes {
            if let Some(layer) = self.load_by_hash(*hash)? {
                dest.insert(*hash, layer);
            }
        }
        Ok(())
    }

    async fn save_by_hash(&mut self, layer: &MerkleLayerContents) -> Result<(), MerkleSerialError> {
        let Keys { payload, children } = Keys::from_hash(layer.payload.hash());
        self.handle
            .insert(payload, layer.payload.bytes().clone())
            .map_err(MerkleSerialError::custom)?;
        self.handle
            .insert(children, render_children(&layer.children))
            .map_err(MerkleSerialError::custom)?;
        Ok(())
    }

    async fn contains_hash(&mut self, hash: Sha256Hash) -> Result<bool, MerkleSerialError> {
        let Keys { payload, children } = Keys::from_hash(hash);
        Ok(self
            .handle
            .contains_key(payload)
            .map_err(MerkleSerialError::custom)?
            && self
                .handle
                .contains_key(children)
                .map_err(MerkleSerialError::custom)?)
    }
}

fn render_children(children: &[Sha256Hash]) -> Vec<u8> {
    let mut v = Vec::with_capacity(children.len() * 32);
    for child in children {
        v.extend_from_slice(child.as_array());
    }
    v
}

fn parse_children(children: &[u8]) -> Result<SmallVec<[Sha256Hash; 16]>, MerkleSerialError> {
    if children.len() % 32 != 0 {
        return Err(MerkleSerialError::custom(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Children in fjall store not a multiple of 32 bytes",
        )));
    }
    let count = children.len() / 32;
    let mut v = SmallVec::with_capacity(count);
    for i in 0..count {
        let start = i * 32;
        let end = start + 32;
        let hash = Sha256Hash::from_array(children[start..end].try_into().unwrap());
        v.push(hash);
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn children_round_trip_helper(payloads: Vec<Vec<u8>>) -> bool {
        let hashes: SmallVec<[Sha256Hash; 16]> = payloads.iter().map(Sha256Hash::hash).collect();
        let rendered = render_children(&hashes);
        let parsed = parse_children(&rendered).unwrap();
        assert_eq!(hashes, parsed);
        true
    }

    quickcheck::quickcheck! {
        fn children_round_trip(payloads: Vec<Vec<u8>>) -> bool {
            children_round_trip_helper(payloads)
        }
    }
}
