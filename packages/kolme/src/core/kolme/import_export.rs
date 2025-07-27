//! Functionality to export blocks to and import blocks from a file.
use std::{
    collections::HashSet,
    io::ErrorKind,
    ops::{Bound, RangeBounds},
    path::Path,
};

use smallvec::SmallVec;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

use crate::*;

impl<App: KolmeApp> Kolme<App> {
    pub async fn export_blocks_to<P: AsRef<Path>, R: RangeBounds<BlockHeight>>(
        &self,
        dest: P,
        range: R,
    ) -> Result<()> {
        let dest = tokio::fs::File::create(dest).await?;
        let mut dest = BufWriter::new(dest);
        let mut stored_hashes = HashSet::new();
        let mut curr = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => start.next(),
            Bound::Unbounded => BlockHeight::start(),
        };

        while range.contains(&curr) {
            tracing::info!("Exporting block {curr}");
            self.export_block(&mut dest, &mut stored_hashes, curr)
                .await?;
            curr = curr.next();
        }

        dest.flush().await?;
        std::mem::drop(dest);

        Ok(())
    }

    async fn export_block(
        &self,
        dest: &mut BufWriter<tokio::fs::File>,
        stored_hashes: &mut std::collections::HashSet<Sha256Hash>,
        height: BlockHeight,
    ) -> Result<()> {
        let block = self.load_block(height).await?;

        let mut work_list = vec![];

        let mut push_with_check = |contents: Arc<MerkleContents>| {
            if !stored_hashes.contains(&contents.hash) {
                let was_added = stored_hashes.insert(contents.hash);
                debug_assert!(was_added);
                work_list.push((contents, true));
            }
        };
        push_with_check(merkle_map::api::serialize(&block.framework_state)?);
        push_with_check(merkle_map::api::serialize(&block.app_state)?);
        push_with_check(merkle_map::api::serialize(&block.logs)?);

        while let Some((contents, check_children)) = work_list.pop() {
            if check_children {
                work_list.push((contents.clone(), false));
                for child in contents.children.iter() {
                    if !stored_hashes.contains(&child.hash) {
                        work_list.push((child.clone(), true));
                        let was_added = stored_hashes.insert(child.hash);
                        debug_assert!(was_added);
                    }
                }
            } else {
                write_layer(dest, &contents).await?;
                debug_assert!(stored_hashes.contains(&contents.hash));
            }
        }
        write_block(dest, &block.block).await?;

        Ok(())
    }

    pub async fn import_blocks_from<P: AsRef<Path>>(&self, src: P) -> Result<()> {
        let src = tokio::fs::File::open(src).await?;
        let mut src = tokio::io::BufReader::new(src);
        loop {
            let b = match src.read_u8().await {
                Ok(b) => b,
                Err(e) => {
                    if e.kind() == ErrorKind::UnexpectedEof {
                        self.resync().await?;
                        break Ok(());
                    } else {
                        break Err(e.into());
                    }
                }
            };

            match b {
                0 => {
                    // Layer
                    let payload_len = usize::try_from(src.read_u32().await?)?;
                    let mut payload = vec![0; payload_len];
                    src.read_exact(&mut payload).await?;
                    let children_len = src.read_u32().await?;
                    let mut children = SmallVec::with_capacity(usize::try_from(children_len)?);
                    for _ in 0..children_len {
                        let mut buff = [0u8; 32];
                        src.read_exact(&mut buff).await?;
                        let hash = Sha256Hash::from_array(buff);
                        anyhow::ensure!(self.has_merkle_hash(hash).await?);
                        children.push(hash);
                    }
                    let hash = Sha256Hash::hash(&payload);
                    let layer = MerkleLayerContents {
                        payload: payload.into(),
                        children,
                    };
                    self.add_merkle_layer(hash, &layer).await?;
                }
                1 => {
                    // Block
                    let len = usize::try_from(src.read_u32().await?)?;
                    let mut buff = vec![0; len];
                    src.read_exact(&mut buff).await?;
                    let block = serde_json::from_slice(&buff)?;
                    self.add_block_with_state(block).await?;
                }
                b => anyhow::bail!("Import blocks failed, found unexpected byte {b}"),
            }
        }
    }
}

async fn write_layer(
    dest: &mut BufWriter<tokio::fs::File>,
    contents: &MerkleContents,
) -> Result<()> {
    dest.write_u8(0).await?;
    dest.write_u32(u32::try_from(contents.payload.len()).context("Payload is too large")?)
        .await?;
    dest.write_all(&contents.payload).await?;
    dest.write_u32(u32::try_from(contents.children.len()).context("Too many children")?)
        .await?;
    for child in contents.children.iter() {
        dest.write_all(child.hash.as_array()).await?;
    }
    Ok(())
}

async fn write_block<AppMessage>(
    dest: &mut BufWriter<tokio::fs::File>,
    block: &SignedBlock<AppMessage>,
) -> Result<()> {
    let serialized = serde_json::to_vec(block)?;
    dest.write_u8(1).await?;
    dest.write_u32(u32::try_from(serialized.len())?).await?;
    dest.write_all(&serialized).await?;
    Ok(())
}
