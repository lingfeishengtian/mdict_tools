use std::io::{Read, Seek, SeekFrom};

use crate::error::{MDictError, Result};

use super::{decode_block, BlockPrefixEntry, PackedStorageHeader};

#[derive(Debug, Clone)]
pub struct PackedStorageIndex {
    pub header: PackedStorageHeader,
    pub data_offset: usize,
    pub base_offset: u64,
}

#[derive(Debug, Clone)]
pub struct DecodedBlock {
    pub block_pos: usize,
    pub uncompressed_start: usize,
    pub uncompressed_end: usize,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanControl {
    Continue { consumed: usize },
    Stop { consumed: usize },
}

#[derive(Debug, Clone, Copy)]
pub struct ReaderBlockPlan {
    pub block_pos: usize,
    pub file_start: u64,
    pub file_end: u64,
    pub uncompressed_start: usize,
    pub uncompressed_end: usize,
}

impl PackedStorageIndex {
    pub fn parse_from_reader<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let base_offset = reader.stream_position()?;
        let (header, data_offset) = PackedStorageHeader::parse_from_reader(reader)?;
        Ok(Self {
            header,
            data_offset,
            base_offset,
        })
    }

    pub fn total_uncompressed_size(&self) -> Option<u64> {
        self.header
            .block_prefix_sum
            .last()
            .map(|entry| entry.uncompressed_end)
    }

    pub fn find_block_pos(&self, uncompressed_offset: u64) -> Option<usize> {
        if self.header.block_prefix_sum.len() < 2 {
            return None;
        }

        let pos = self
            .header
            .block_prefix_sum
            .partition_point(|entry| entry.uncompressed_end <= uncompressed_offset);

        if pos == 0 || pos >= self.header.block_prefix_sum.len() {
            return None;
        }

        Some(pos)
    }

    fn block_bounds(&self, block_pos: usize) -> Option<(BlockPrefixEntry, BlockPrefixEntry)> {
        if block_pos == 0 || block_pos >= self.header.block_prefix_sum.len() {
            return None;
        }

        let prev = self.header.block_prefix_sum[block_pos - 1];
        let cur = self.header.block_prefix_sum[block_pos];
        Some((prev, cur))
    }

    pub fn index_block_for_reader(&self, block_pos: usize) -> Result<ReaderBlockPlan> {
        let (prev, cur) = self.block_bounds(block_pos).ok_or_else(|| {
            MDictError::InvalidArgument(format!("invalid block position: {}", block_pos))
        })?;

        if cur.compressed_end < prev.compressed_end || cur.uncompressed_end < prev.uncompressed_end {
            return Err(MDictError::InvalidFormat(
                "non-monotonic block bounds".to_string(),
            ));
        }

        let compressed_start = usize::try_from(prev.compressed_end)
            .map_err(|_| MDictError::InvalidFormat("compressed_start overflow".to_string()))?;
        let compressed_end = usize::try_from(cur.compressed_end)
            .map_err(|_| MDictError::InvalidFormat("compressed_end overflow".to_string()))?;
        let uncompressed_start = usize::try_from(prev.uncompressed_end)
            .map_err(|_| MDictError::InvalidFormat("uncompressed_start overflow".to_string()))?;
        let uncompressed_end = usize::try_from(cur.uncompressed_end)
            .map_err(|_| MDictError::InvalidFormat("uncompressed_end overflow".to_string()))?;

        let relative_data_offset = u64::try_from(self.data_offset)
            .map_err(|_| MDictError::InvalidFormat("data_offset overflow".to_string()))?;
        let file_start = self
            .base_offset
            .checked_add(relative_data_offset)
            .and_then(|x| x.checked_add(compressed_start as u64))
            .ok_or_else(|| MDictError::InvalidFormat("block start offset overflow".to_string()))?;
        let file_end = self
            .base_offset
            .checked_add(relative_data_offset)
            .and_then(|x| x.checked_add(compressed_end as u64))
            .ok_or_else(|| MDictError::InvalidFormat("block end offset overflow".to_string()))?;

        if file_end < file_start {
            return Err(MDictError::InvalidFormat(
                "invalid compressed bounds".to_string(),
            ));
        }

        Ok(ReaderBlockPlan {
            block_pos,
            uncompressed_start,
            uncompressed_end,
            file_start,
            file_end,
        })
    }

    pub fn index_block_at_offset_for_reader(&self, offset: u64) -> Result<Option<ReaderBlockPlan>> {
        let Some(block_pos) = self.find_block_pos(offset) else {
            return Ok(None);
        };
        self.index_block_for_reader(block_pos).map(Some)
    }

    pub fn decode_block_from_reader<R: Read + Seek>(
        &self,
        reader: &mut R,
        block_pos: usize,
    ) -> Result<DecodedBlock> {
        let plan = self.index_block_for_reader(block_pos)?;

        let compressed_size = usize::try_from(plan.file_end - plan.file_start)
            .map_err(|_| MDictError::InvalidFormat("compressed size overflow".to_string()))?;
        let mut compressed = vec![0u8; compressed_size];
        reader.seek(SeekFrom::Start(plan.file_start))?;
        reader.read_exact(&mut compressed)?;

        let expected_size = plan.uncompressed_end - plan.uncompressed_start;
        let bytes = decode_block(self.header.encoding, &compressed, expected_size)?;
        println!("Decoded block {}: compressed {} bytes to {} bytes", block_pos, compressed.len(), bytes.len());

        Ok(DecodedBlock {
            block_pos: plan.block_pos,
            uncompressed_start: plan.uncompressed_start,
            uncompressed_end: plan.uncompressed_end,
            bytes,
        })
    }

    pub fn decode_block_at_offset_from_reader<R: Read + Seek>(
        &self,
        reader: &mut R,
        offset: u64,
    ) -> Result<Option<DecodedBlock>> {
        let Some(plan) = self.index_block_at_offset_for_reader(offset)? else {
            return Ok(None);
        };
        self.decode_block_from_reader(reader, plan.block_pos).map(Some)
    }

    pub fn read_from_offset_with_options<R: Read + Seek>(
        &self,
        reader: &mut R,
        start_offset: u64,
        terminator: Option<&[u8]>,
        record_size: Option<u64>,
    ) -> Result<Vec<u8>> {
        if terminator.is_none() && record_size.is_none() {
            return Err(MDictError::InvalidArgument(
                "either terminator or record_size must be provided".to_string(),
            ));
        }

        if let Some(term) = terminator {
            if term.is_empty() {
                return Err(MDictError::InvalidArgument(
                    "terminator must not be empty".to_string(),
                ));
            }
        }

        let total_uncompressed = self.total_uncompressed_size().ok_or_else(|| {
            MDictError::InvalidFormat("missing total uncompressed size".to_string())
        })?;

        if start_offset >= total_uncompressed {
            return Err(MDictError::InvalidArgument(format!(
                "start_offset {} is out of bounds for total size {}",
                start_offset, total_uncompressed
            )));
        }

        let mut out = Vec::new();
        let mut current_offset = start_offset;
        let mut remaining = record_size;

        while current_offset < total_uncompressed {
            if matches!(remaining, Some(0)) {
                break;
            }

            let Some(decoded_block) = self.decode_block_at_offset_from_reader(reader, current_offset)? else {
                break;
            };

            let local_start = usize::try_from(current_offset)
                .ok()
                .and_then(|absolute| absolute.checked_sub(decoded_block.uncompressed_start))
                .ok_or_else(|| MDictError::InvalidFormat("local offset overflow".to_string()))?;

            if local_start > decoded_block.bytes.len() {
                return Err(MDictError::InvalidFormat(
                    "local start exceeds decoded block size".to_string(),
                ));
            }

            let chunk = &decoded_block.bytes[local_start..];
            if chunk.is_empty() {
                current_offset = decoded_block.uncompressed_end as u64;
                continue;
            }

            let mut take = chunk.len();
            if let Some(left) = remaining {
                let left_usize = usize::try_from(left.min(usize::MAX as u64))
                    .map_err(|_| MDictError::InvalidFormat("record size overflow".to_string()))?;
                take = take.min(left_usize);
            }

            if take == 0 {
                break;
            }

            let prev_len = out.len();
            out.extend_from_slice(&chunk[..take]);
            current_offset = current_offset.saturating_add(take as u64);

            if let Some(left) = remaining.as_mut() {
                *left = left.saturating_sub(take as u64);
            }

            if let Some(term) = terminator {
                let search_from = prev_len.saturating_sub(term.len().saturating_sub(1));
                if let Some(pos_rel) = out[search_from..]
                    .windows(term.len())
                    .position(|window| window == term)
                {
                    let term_pos = search_from + pos_rel;
                    out.truncate(term_pos);
                    return Ok(out);
                }
            }

            if matches!(remaining, Some(0)) {
                break;
            }
        }

        Ok(out)
    }
}
