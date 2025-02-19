use crate::file_reader::FileHandler;

use super::parser::{KeyBlock, KeySection};

pub struct SearchResultPointer {
    start_key_index: u64,
    end_key_index: u64,
    start_key_block_offset: u64,
    end_key_block_offset: u64,
    current_key_index_offset: u64,
    current_key_block_index: u64,
}

impl SearchResultPointer {
    pub fn new(start_key_index: usize, end_key_index: usize, start_key_block_offset: u64, end_key_block_offset: u64) -> Self {
        Self {
            start_key_index: start_key_index as u64,
            end_key_index: end_key_index as u64,
            start_key_block_offset,
            end_key_block_offset,
            current_key_index_offset: start_key_index as u64,
            current_key_block_index: start_key_block_offset
        }
    }

    pub fn next(&mut self, file_handler: &mut FileHandler, key_section: &mut KeySection) -> Option<KeyBlock> {
        if self.current_key_index_offset > self.end_key_index || (self.current_key_index_offset == self.end_key_index && self.current_key_block_index >= self.end_key_block_offset) {
            return None;
        }

        let current_key_entries = key_section.key_index(self.current_key_index_offset).num_entries;
        let block = key_section.read_block_index(file_handler, self.current_key_index_offset, self.current_key_block_index);

        if self.current_key_block_index < current_key_entries - 1 {
            self.current_key_block_index += 1;
        } else {
            self.current_key_block_index = 0;
            self.current_key_index_offset += 1;
        }

        Some(block)
    }
}
