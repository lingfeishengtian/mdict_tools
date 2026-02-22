use std::io::{Read, Seek, SeekFrom};

use crate::error::Result;
use crate::format::{HeaderInfo, KeySection};
use crate::types::KeyBlock;

pub struct KeyBlockIndex {
    pub header: HeaderInfo,
    pub key_section: KeySection,
    pub key_blocks_start: u64,

    cached_block_idx: Option<usize>,
    cached_entries: Option<Vec<KeyBlock>>,
    read_buf: Vec<u8>,
}

impl KeyBlockIndex {
    pub fn new(header: HeaderInfo, key_section: KeySection) -> Result<Self> {
        let total_key_blocks_size = *key_section.key_info_prefix_sum.last().unwrap_or(&0);

        let key_blocks_start = key_section.next_section_offset - total_key_blocks_size;

        Ok(Self {
            header,
            key_section,
            key_blocks_start,
            cached_block_idx: None,
            cached_entries: None,
            read_buf: Vec::new(),
        })
    }

    /// Ensure the requested block is decoded and cached, returning a reference
    /// to the cached entries.
    fn load_block(
        &mut self,
        reader: &mut (impl Read + Seek),
        idx: usize,
    ) -> Result<&Vec<KeyBlock>> {
        if self.cached_block_idx == Some(idx) {
            return Ok(self.cached_entries.as_ref().unwrap());
        }

        let kb = &self.key_section.key_info_blocks[idx];
        let offset = self.key_blocks_start + self.key_section.key_info_prefix_sum[idx];
        let size = kb.compressed_size as usize;

        self.read_buf.clear();
        self.read_buf.resize(size, 0);

        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut self.read_buf)?;

        let decoded = crate::format::decode_format_block(&self.read_buf)?;
        let entries = crate::format::parse_key_block(&decoded, self.header.get_encoding())?;

        self.cached_entries = Some(entries);
        self.cached_block_idx = Some(idx);

        Ok(self.cached_entries.as_ref().unwrap())
    }

    fn find_candidate_block_for_prefix(&self, prefix: &str) -> Option<(usize, usize)> {
        let blocks = &self.key_section.key_info_blocks;
        let idx = blocks.partition_point(|b| b.last.as_str() < prefix);
        let upper_bound_prefix = upper_bound_from_prefix(prefix).or_else(|| Some(prefix.to_string()))?;
        let idx_upper = blocks.partition_point(|b| b.last.as_str() < upper_bound_prefix.as_str());

        if idx >= blocks.len() {
            None
        } else {
            Some((idx, idx_upper))
        }
    }

    pub fn get(&mut self, reader: &mut (impl Read + Seek), idx: usize) -> Result<Option<KeyBlock>> {
        let block_idx = self
            .key_section
            .num_entries_prefix_sum
            .partition_point(|&x| x <= idx as u64);

        if block_idx > self.key_section.key_info_blocks.len() {
            return Ok(None);
        }

        // TODO: I know we can make this logic cleaner by fixing the prefix sum to have 0 first
        let num_entries_prefix_sum = if block_idx == 0 {
            0
        } else {
            self.key_section.num_entries_prefix_sum[block_idx - 1]
        };

        let block = self.load_block(reader, block_idx - 1)?;
        let offset = idx - num_entries_prefix_sum as usize;

        Ok(block.get(offset).cloned())
    }

    pub fn index_for(&mut self, reader: &mut (impl Read + Seek), key_text: &str) -> Result<Option<usize>> {
        let blocks = &self.key_section.key_info_blocks;
        let block_idx = blocks.partition_point(|b| b.last.as_str() < key_text);

        if block_idx > blocks.len() {
            return Ok(None);
        }

        let block = self.load_block(reader, block_idx)?;
        let entry_idx = block.partition_point(|e| e.key_text.as_str() < key_text);

        if entry_idx == block.len() || block[entry_idx].key_text != key_text {
            Ok(None)
        } else {
            let global_idx = self.key_section.num_entries_prefix_sum[block_idx] as usize + entry_idx;
            Ok(Some(global_idx))
        }
    }

    pub fn prefix_range_bounds(
        &mut self,
        reader: &mut (impl Read + Seek),
        prefix: &str,
    ) -> Result<Option<(usize, usize)>> {
        // Find the candidate block that might contain keys with this prefix
        let (lower_bound, upper_bound) = match self.find_candidate_block_for_prefix(prefix) {
            Some(idx) => idx,
            None => return Ok(None),
        };

        let block_entries_lower = self.key_section.num_entries_prefix_sum[lower_bound] as usize;
        let block_entries_upper = self.key_section.num_entries_prefix_sum[upper_bound] as usize;

        let entries_lower = self.load_block(reader, lower_bound)?;
        let lower_bound_pos = entries_lower.partition_point(|e| e.key_text.as_str() < prefix);

        if lower_bound_pos >= entries_lower.len() {
            return Ok(None);
        }

        let entries_upper = self.load_block(reader, upper_bound)?;
        let upper_bound_prefix = upper_bound_from_prefix(prefix).unwrap_or_else(|| prefix.to_string());
        let upper_bound_pos = entries_upper.partition_point(|e| e.key_text.as_str() < upper_bound_prefix.as_str());

        let lower_index = block_entries_lower + lower_bound_pos;
        let upper_index = block_entries_upper + upper_bound_pos;

        Ok(Some((lower_index, upper_index)))
    }
}

fn upper_bound_from_prefix(prefix: &str) -> Option<String> {
    for i in (0..prefix.len()).rev() {
        if let Some(last_char_str) = prefix.get(i..) {
            let rest_of_prefix = {
                debug_assert!(prefix.is_char_boundary(i));
                &prefix[0..i]
            };

            let last_char = last_char_str
                .chars()
                .next()
                .expect("last_char_str will contain at least one char");
            let Some(last_char_incr) = (last_char ..= char::MAX).nth(1) else {
                // Last character is highest possible code point.
                // Go to second-to-last character instead.
                continue;
            };
            
            let new_string = format!("{rest_of_prefix}{last_char_incr}");

            return Some(new_string);
        }
    }

    None
}