use std::io::{Seek, Write};

use crate::error::{MDictError, Result};

use super::{encode_block, BlockPrefixEntry, CompressionEncoding, PackedStorageHeader};

pub struct PackedStorageWriter {
    header: PackedStorageHeader,
    target_uncompressed_block_size: usize,
    pending_block: Vec<u8>,
    compressed_blocks: Vec<Vec<u8>>,
}

impl PackedStorageWriter {
    pub fn new(
        encoding: CompressionEncoding,
        compression_level: u8,
        target_uncompressed_block_size: usize,
    ) -> Result<Self> {
        if target_uncompressed_block_size == 0 {
            return Err(MDictError::InvalidArgument(
                "target_uncompressed_block_size must be > 0".to_string(),
            ));
        }

        Ok(Self {
            header: PackedStorageHeader {
                encoding,
                compression_level,
                num_entries: 0,
                block_prefix_sum: vec![BlockPrefixEntry {
                    compressed_end: 0,
                    uncompressed_end: 0,
                }],
            },
            target_uncompressed_block_size,
            pending_block: Vec::new(),
            compressed_blocks: Vec::new(),
        })
    }

    fn flush_pending_block(&mut self) -> Result<()> {
        if self.pending_block.is_empty() {
            return Ok(());
        }

        let compressed = encode_block(
            self.header.encoding,
            self.header.compression_level,
            &self.pending_block,
        )?;

        let last_prefix = self.header.block_prefix_sum.last().copied().ok_or_else(|| {
            MDictError::InvalidFormat("missing initial prefix entry".to_string())
        })?;

        let compressed_end = last_prefix
            .compressed_end
            .checked_add(compressed.len() as u64)
            .ok_or_else(|| MDictError::InvalidFormat("compressed size overflow".to_string()))?;
        let uncompressed_end = last_prefix
            .uncompressed_end
            .checked_add(self.pending_block.len() as u64)
            .ok_or_else(|| MDictError::InvalidFormat("uncompressed size overflow".to_string()))?;

        self.header.block_prefix_sum.push(BlockPrefixEntry {
            compressed_end,
            uncompressed_end,
        });

        self.compressed_blocks.push(compressed);
        self.pending_block.clear();
        Ok(())
    }

    pub fn push_entry(&mut self, entry: &[u8]) -> Result<u64> {
        if !self.pending_block.is_empty()
            && self.pending_block.len() + entry.len() > self.target_uncompressed_block_size
        {
            self.flush_pending_block()?;
        }

        let total_uncompressed = self
            .header
            .block_prefix_sum
            .last()
            .map(|entry| entry.uncompressed_end)
            .ok_or_else(|| MDictError::InvalidFormat("missing initial prefix entry".to_string()))?;

        let offset = total_uncompressed
            .checked_add(self.pending_block.len() as u64)
            .ok_or_else(|| MDictError::InvalidFormat("uncompressed offset overflow".to_string()))?;

        self.pending_block.extend_from_slice(entry);
        self.header.num_entries += 1;
        Ok(offset)
    }

    pub fn finish_into_bytes(mut self) -> Result<Vec<u8>> {
        self.flush_pending_block()?;

        let mut out = self.header.write_to_bytes()?;
        for block in self.compressed_blocks {
            out.extend_from_slice(&block);
        }
        Ok(out)
    }

    pub fn finish_to_writer<W: Write + Seek>(self, writer: &mut W) -> Result<()> {
        let bytes = self.finish_into_bytes()?;
        writer.write_all(&bytes)?;
        Ok(())
    }
}
