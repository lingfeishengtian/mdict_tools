use std::io::{Read, Seek, SeekFrom};

use crate::error::Result;
use crate::format::{HeaderInfo, KeySection};
use crate::types::KeyBlock;

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}
pub struct KeyBlockIndex<'a> {
    reader: &'a mut dyn ReadSeek,
    header: &'a HeaderInfo,
    key_section: &'a KeySection,
    key_blocks_start: u64,

    cached_block_idx: Option<usize>,
    cached_entries: Option<Vec<KeyBlock>>,
    read_buf: Vec<u8>,
}

impl<'a> KeyBlockIndex<'a> {
    pub fn new(
        reader: &'a mut dyn ReadSeek,
        header: &'a HeaderInfo,
        key_section: &'a KeySection,
    ) -> Result<Self> {
        let total_key_blocks_size = *key_section.key_info_prefix_sum.last().unwrap_or(&0);

        let key_blocks_start = key_section.next_section_offset - total_key_blocks_size;

        Ok(Self {
            reader,
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
    fn load_block(&mut self, idx: usize) -> Result<&Vec<KeyBlock>> {
        if self.cached_block_idx == Some(idx) {
            return Ok(self.cached_entries.as_ref().unwrap());
        }

        let kb = &self.key_section.key_info_blocks[idx];
        let offset = self.key_blocks_start + self.key_section.key_info_prefix_sum[idx];
        let size = kb.compressed_size as usize;

        self.read_buf.clear();
        self.read_buf.resize(size, 0);

        self.reader.seek(SeekFrom::Start(offset))?;
        self.reader.read_exact(&mut self.read_buf)?;

        let decoded = crate::format::decode_format_block(&self.read_buf)?;
        let entries = crate::format::parse_key_block(&decoded, self.header.get_encoding())?;

        self.cached_entries = Some(entries);
        self.cached_block_idx = Some(idx);

        Ok(self.cached_entries.as_ref().unwrap())
    }

    fn find_candidate_block(&self, key: &str) -> Option<usize> {
        let blocks = &self.key_section.key_info_blocks;
        let idx = blocks.partition_point(|b| b.last.as_str() < key);
        if idx >= blocks.len() {
            None
        } else {
            Some(idx)
        }
    }

    /// Helper: find the candidate block for `key` and return a reference
    /// to the (cached) entries for that block.
    fn entries_for_key(&mut self, key: &str) -> Result<Option<(usize, &Vec<KeyBlock>)>> {
        let block_idx = match self.find_candidate_block(key) {
            Some(idx) => idx,
            None => return Ok(None),
        };

        let entries = self.load_block(block_idx)?;
        Ok(Some((block_idx, entries)))
    }

    fn entries_for_exact_key(&mut self, key: &str) -> Result<Option<(usize, &Vec<KeyBlock>)>> {
        let block_idx = match self.find_candidate_block(key) {
            Some(idx) => idx,
            None => return Ok(None),
        };

        let kb_info = &self.key_section.key_info_blocks[block_idx];

        if kb_info.first.as_str() > key {
            return Ok(None);
        }

        let entries = self.load_block(block_idx)?;
        Ok(Some((block_idx, entries)))
    }

    pub fn get(&mut self, key: &str) -> Result<Option<KeyBlock>> {
        let (_block_idx, entries) = match self.entries_for_exact_key(key)? {
            Some(t) => t,
            None => return Ok(None),
        };

        let pos = entries.partition_point(|e| e.key_text.as_str() < key);
        if pos < entries.len() && entries[pos].key_text.as_str() == key {
            Ok(Some(entries[pos].clone()))
        } else {
            Ok(None)
        }
    }

    pub fn get_as_index(&mut self, key: &str) -> Result<Option<usize>> {
        let (block_idx, entries) = match self.entries_for_exact_key(key)? {
            Some(t) => t,
            None => return Ok(None),
        };

        let pos = entries.partition_point(|e| e.key_text.as_str() < key);

        if pos < entries.len() && entries[pos].key_text.as_str() == key {
            let index_before = self.key_section.key_info_prefix_sum[block_idx] as usize;
            Ok(Some(index_before + pos))
        } else {
            Ok(None)
        }
    }

    pub fn lower_bound(&mut self, key: &str) -> Result<Option<KeyBlock>> {
        let (_block_idx, entries) = match self.entries_for_key(key)? {
            Some(t) => t,
            None => return Ok(None),
        };

        let pos = entries.partition_point(|e| e.key_text.as_str() < key);

        if pos < entries.len() {
            Ok(Some(entries[pos].clone()))
        } else {
            Ok(None)
        }
    }

    pub fn prefix_range_bounds(
        &mut self,
        prefix: &str,
    ) -> Result<(Option<KeyBlock>, Option<KeyBlock>)> {
        // Find the candidate block that might contain keys with this prefix
        let (block_idx, entries) = match self.entries_for_key(prefix)? {
            Some((idx, entries)) => (idx, entries),
            None => return Ok((None, None)),
        };

        // Find first key >= prefix
        let lower_bound_pos = entries.partition_point(|e| e.key_text.as_str() < prefix);

        // If no keys are >= prefix, return None for both bounds
        if lower_bound_pos >= entries.len() {
            return Ok((None, None));
        }

        // Get the first key that is >= prefix (lower bound)
        let lower_key = entries[lower_bound_pos].clone();

        // Find first key that does NOT start with prefix
        let upper_bound_pos = entries[lower_bound_pos..]
            .partition_point(|e| e.key_text.as_str().starts_with(prefix))
            + lower_bound_pos;

        // Get the first key that doesn't start with prefix (upper bound)
        let upper_key = if upper_bound_pos < entries.len() {
            Some(entries[upper_bound_pos].clone())
        } else {
            None
        };

        Ok((Some(lower_key), upper_key))
    }

    fn find_block_by_entry_index(&self, index: usize) -> Option<(usize, usize)> {
        let prefix = &self.key_section.key_info_prefix_sum;

        let block = prefix.partition_point(|&x| x <= index as u64);

        if block >= prefix.len() {
            return None;
        }

        let block_start = if block == 0 {
            0
        } else {
            prefix[block - 1] as usize
        };

        let offset_in_block = index - block_start;

        Some((block, offset_in_block))
    }

    pub fn get_by_index(&mut self, index: usize) -> Result<Option<KeyBlock>> {
        let (block_idx, offset) = match self.find_block_by_entry_index(index) {
            Some(v) => v,
            None => return Ok(None),
        };

        let entries = self.load_block(block_idx)?;

        Ok(entries.get(offset).cloned())
    }
}
