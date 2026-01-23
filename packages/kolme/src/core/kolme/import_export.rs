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

#[derive(thiserror::Error, Debug)]
pub enum KolmeImportExportError {
    #[error("Child hash {child} was not previously written")]
    ChildHashNotPreviouslyWritten { child: Sha256Hash },

    #[error("Merkle hash {child} not found in Merkle store for parent {parent}")]
    MissingMerkleHashInStore {
        child: Sha256Hash,
        parent: Sha256Hash,
    },

    #[error("Missing layer {hash}")]
    MissingLayer { hash: Sha256Hash },

    #[error("Logic error: writing layer {parent} but its child {child} is not yet written")]
    LogicChildNotYetWritten {
        parent: Sha256Hash,
        child: Sha256Hash,
    },

    #[error("Import blocks failed, found unexpected byte {byte}")]
    UnexpectedByte { byte: u8 },

    #[error("Payload is too large")]
    PayloadTooLarge,

    #[error("Too many children")]
    TooManyChildren,
}

impl<App: KolmeApp> Kolme<App> {
    pub async fn export_blocks_to<P: AsRef<Path>, R: RangeBounds<BlockHeight>>(
        &self,
        dest: P,
        range: R,
    ) -> Result<(), KolmeError> {
        let dest = tokio::fs::File::create(dest).await?;
        let mut dest = BufWriter::new(dest);
        let mut curr = match range.start_bound() {
            Bound::Included(start) => *start,
            Bound::Excluded(start) => start.next(),
            Bound::Unbounded => BlockHeight::start(),
        };
        let mut written_layers = HashSet::new();

        while range.contains(&curr) {
            tracing::info!("Exporting block {curr}");
            self.export_block(&mut dest, &mut written_layers, curr)
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
        written_layers: &mut std::collections::HashSet<Sha256Hash>,
        height: BlockHeight,
    ) -> Result<(), KolmeError> {
        let block = self.load_block(height).await?;

        enum Work {
            Process(Sha256Hash),
            Write(Sha256Hash, Box<MerkleLayerContents>),
        }
        let mut work_queue = vec![
            Work::Process(block.block.as_inner().framework_state),
            Work::Process(block.block.as_inner().app_state),
            Work::Process(block.block.as_inner().logs),
        ];

        while let Some(work) = work_queue.pop() {
            match work {
                Work::Process(hash) => {
                    if written_layers.contains(&hash) {
                        continue;
                    }
                    let layer = self
                        .get_merkle_layer(hash)
                        .await?
                        .ok_or(KolmeImportExportError::MissingLayer { hash })?;
                    let children = layer.children.clone();
                    work_queue.push(Work::Write(hash, Box::new(layer)));
                    for child in children {
                        if !written_layers.contains(&child) {
                            work_queue.push(Work::Process(child));
                        }
                    }
                }
                Work::Write(hash, layer) => {
                    if written_layers.contains(&hash) {
                        continue;
                    }
                    write_layer(dest, hash, &layer, written_layers).await?;
                }
            }
        }
        write_block(dest, &block.block).await?;

        Ok(())
    }

    pub async fn import_blocks_from<P: AsRef<Path>>(&self, src: P) -> Result<(), KolmeError> {
        let src = tokio::fs::File::open(src).await?;
        let mut src = tokio::io::BufReader::new(src);
        let mut hashes = HashSet::new();
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
                    let payload = CachedBytes::new_bytes(payload);
                    for _ in 0..children_len {
                        let mut buff = [0u8; 32];
                        src.read_exact(&mut buff).await?;
                        let child = Sha256Hash::from_array(buff);

                        if !hashes.contains(&child) {
                            return Err(KolmeImportExportError::ChildHashNotPreviouslyWritten {
                                child,
                            }
                            .into());
                        }

                        let parent = payload.hash();
                        if !self.has_merkle_hash(child).await? {
                            return Err(KolmeImportExportError::MissingMerkleHashInStore {
                                child,
                                parent,
                            }
                            .into());
                        }
                        children.push(child);
                    }
                    let hash = payload.hash();
                    let layer = MerkleLayerContents { payload, children };
                    self.add_merkle_layer(&layer).await?;
                    hashes.insert(hash);
                }
                1 => {
                    // Block
                    let len = usize::try_from(src.read_u32().await?)?;
                    let mut buff = vec![0; len];
                    src.read_exact(&mut buff).await?;
                    let block: Arc<SignedBlock<App::Message>> = serde_json::from_slice(&buff)?;
                    let height = block.height();
                    if self.has_block(height).await? {
                        tracing::info!("Block height {height} already present");
                    } else {
                        tracing::info!("Writing block {height}");
                        self.add_block_with_state(block).await?;
                    }
                }
                b => return Err(KolmeImportExportError::UnexpectedByte { byte: b }.into()),
            }
        }
    }
}

async fn write_layer(
    dest: &mut BufWriter<tokio::fs::File>,
    hash: Sha256Hash,
    layer: &MerkleLayerContents,
    written_layers: &mut HashSet<Sha256Hash>,
) -> Result<(), KolmeError> {
    dest.write_u8(0).await?;
    dest.write_u32(
        u32::try_from(layer.payload.len()).map_err(|_| KolmeImportExportError::PayloadTooLarge)?,
    )
    .await?;
    dest.write_all(layer.payload.bytes()).await?;
    dest.write_u32(
        u32::try_from(layer.children.len()).map_err(|_| KolmeImportExportError::TooManyChildren)?,
    )
    .await?;
    for child in &layer.children {
        if !written_layers.contains(child) {
            return Err(KolmeImportExportError::LogicChildNotYetWritten {
                parent: hash,
                child: *child,
            }
            .into());
        }
        dest.write_all(child.as_array()).await?;
    }
    written_layers.insert(hash);
    Ok(())
}

async fn write_block<AppMessage>(
    dest: &mut BufWriter<tokio::fs::File>,
    block: &SignedBlock<AppMessage>,
) -> Result<(), KolmeError> {
    let serialized = serde_json::to_vec(block)?;
    dest.write_u8(1).await?;
    dest.write_u32(u32::try_from(serialized.len())?).await?;
    dest.write_all(&serialized).await?;
    Ok(())
}
