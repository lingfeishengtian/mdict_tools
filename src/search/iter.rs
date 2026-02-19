use std::io::{Read, Seek, SeekFrom};

use crate::error::Result;
use crate::types::KeyBlock;
use crate::format::{HeaderInfo, KeySection};

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Iterator over KeyBlock entries reading blocks lazily from the underlying reader.
///
/// The iterator yields `Result<KeyBlock>` so IO/decoding errors are propagated.
pub struct KeyBlocksIterator<'a> {
    pub reader: &'a mut dyn ReadSeek,
    pub header: &'a HeaderInfo,
    pub key_section: &'a KeySection,
    pub key_blocks_start: u64,

    pub block_idx: usize,
    pub entries: Vec<KeyBlock>,
    pub entry_idx: usize,
}

impl<'a> KeyBlocksIterator<'a> {
    /// Create an iterator starting at `start_block_idx` (block index into `key_info_blocks`).
    pub fn new(
        reader: &'a mut dyn ReadSeek,
        header: &'a HeaderInfo,
        key_section: &'a KeySection,
        start_block_idx: usize,
    ) -> Result<Self> {
        let total_key_blocks_size = *key_section.key_info_prefix_sum.last().unwrap_or(&0u64);
        let key_blocks_start = key_section.next_section_offset - total_key_blocks_size;

        let mut it = KeyBlocksIterator {
            reader,
            header,
            key_section,
            key_blocks_start,
            block_idx: start_block_idx,
            entries: Vec::new(),
            entry_idx: 0,
        };

        it.load_next_nonempty_block()?;
        Ok(it)
    }

    /// Binary-search helper: find the block index that may contain `key_text`.
    /// It returns the first block whose `last >= key_text` (or `num_blocks` if none).
    pub fn find_block_for_key(key_section: &KeySection, key_text: &str) -> usize {
        let blocks = &key_section.key_info_blocks;
        blocks.partition_point(|b| b.last.as_str() < key_text)
    }

    fn load_block(&mut self, idx: usize) -> Result<()> {
        if idx >= self.key_section.key_info_blocks.len() {
            self.entries.clear();
            self.entry_idx = 0;
            return Ok(());
        }

        let kb = &self.key_section.key_info_blocks[idx];
        let offset = self.key_blocks_start + self.key_section.key_info_prefix_sum[idx];
        let size = kb.compressed_size as usize;
        let mut buf = vec![0u8; size];
        self.reader.seek(SeekFrom::Start(offset))?;
        self.reader.read_exact(&mut buf)?;

        let decoded = crate::format::decode_format_block(&buf)?;
        let entries = crate::format::parse_key_block(&decoded, self.header.get_encoding())?;

        self.entries = entries;
        self.entry_idx = 0;
        Ok(())
    }

    fn load_next_nonempty_block(&mut self) -> Result<()> {
        while self.block_idx < self.key_section.key_info_blocks.len() {
            self.load_block(self.block_idx)?;
            if !self.entries.is_empty() {
                return Ok(());
            }
            self.block_idx += 1;
        }
        Ok(())
    }
}

impl<'a> Iterator for KeyBlocksIterator<'a> {
    type Item = Result<KeyBlock>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.entry_idx < self.entries.len() {
            let kb = self.entries[self.entry_idx].clone();
            self.entry_idx += 1;
            return Some(Ok(kb));
        }

        self.block_idx += 1;
        if self.block_idx >= self.key_section.key_info_blocks.len() {
            return None;
        }

        if let Err(e) = self.load_next_nonempty_block() {
            return Some(Err(e));
        }

        if self.entry_idx < self.entries.len() {
            let kb = self.entries[self.entry_idx].clone();
            self.entry_idx += 1;
            Some(Ok(kb))
        } else {
            None
        }
    }
}

/// Convenience: create an iterator starting from the block likely to contain `key_text`.
/// This uses a binary search over block `last` values to avoid scanning many blocks.
pub fn iterator_from_key<'a>(
    reader: &'a mut dyn ReadSeek,
    header: &'a HeaderInfo,
    key_section: &'a KeySection,
    key_text: &str,
) -> Result<KeyBlocksIterator<'a>> {
    let idx = KeyBlocksIterator::find_block_for_key(key_section, key_text);
    let num_blocks = key_section.key_info_blocks.len();
    let start = if idx >= num_blocks { num_blocks } else { idx };

    KeyBlocksIterator::new(reader, header, key_section, start)
}
