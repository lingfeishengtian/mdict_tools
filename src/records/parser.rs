use crate::{compressed_block::block::decode_block, file_reader::FileHandler, header::parser::{HeaderInfo, MdictVersion}, key_index::{self, parser::KeySection}, shared_macros::read_int_from_buf};
use std::collections::VecDeque;

pub struct RecordSection {
    record_data_offset: u64,
    record_index_prefix_sum: Vec<RecordIndex>,
    cache: VecDeque<(u64, Vec<u8>)>, // Cache to store the 4 most used records
}

#[derive(Clone)]
pub struct RecordIndex {
    compressed_size: u64,
    uncompressed_size: u64,
}

impl RecordSection {
    pub fn parse(header_index: &HeaderInfo, key_index: &KeySection, file_handler: &mut FileHandler) -> RecordSection {
        let mut record_data_offset  = key_index.next_section_offset();
        let record_index_prefix_sum = RecordSection::create_record_index(header_index, file_handler, &mut record_data_offset);

        RecordSection {
            record_data_offset,
            record_index_prefix_sum,
            cache: VecDeque::with_capacity(5),
        }
    }

    fn decode_record_data(&mut self, record_index: u64, file_handler: &mut FileHandler) {
        // If in cache move to front
        for i in 0..self.cache.len() {
            if self.cache[i].0 == record_index {
                let record = self.cache.remove(i).unwrap();
                self.cache.push_front(record);
                return;
            }
        }

        // If cache is full remove the last element
        if self.cache.len() == 4 {
            self.cache.pop_back();
        }

        println!("Record index cache miss i: {}", record_index);
        let size_of_compressed = self.record_index_prefix_sum[record_index as usize + 1].compressed_size - self.record_index_prefix_sum[record_index as usize].compressed_size;
        let mut record_data = vec![0; size_of_compressed as usize];
        file_handler.read_from_file(self.record_data_offset + self.record_index_prefix_sum[record_index as usize].compressed_size, &mut record_data).unwrap();

        record_data = decode_block(&record_data).unwrap();
        
        // Add to cache
        self.cache.push_front((record_index, record_data));
    }

    pub fn record_at_offset(&mut self, offset: u64, file_handler: &mut FileHandler) -> String {
        let record_index_i = self.bin_search_record_index(offset);
        self.decode_record_data(record_index_i, file_handler);
        let record_data = self.cache.front().unwrap().1.as_slice();

        let record_index = &self.record_index_prefix_sum[record_index_i as usize];
        let decompressed_offset = (offset - record_index.uncompressed_size) as usize;

        // Return until 0x0A 0x00
        let mut record_text = Vec::new();
        for i in decompressed_offset..record_data.len() {
            if record_data[i] == 0x0A && record_data[i + 1] == 0x00 {
                break;
            }

            record_text.push(record_data[i]);
        }
        // TODO: Change to encoding in header
        let record = std::str::from_utf8(&record_text).unwrap().to_string();

        record
    }

    // Return the index of the record index that contains the offset
    // Resulting index is the greatest index such that record_index_prefix_sum[index].uncompressed <= offset
    fn bin_search_record_index(&self, offset: u64) -> u64 {
        let mut left = 0;
        let mut right = self.record_index_prefix_sum.len() as u64 - 1;

        while left < right {
            let mid = left + (right - left) / 2;
            if self.record_index_prefix_sum[mid as usize].uncompressed_size <= offset {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        left - 1
    }

    fn create_record_index(header_index: &HeaderInfo, file_handler: &mut FileHandler, offset: &mut u64) -> Vec<RecordIndex> {
        let read_size = match header_index.get_version() {
            MdictVersion::V1 => 4,
            MdictVersion::V2 => 8,
            MdictVersion::V3 => 0
        };

        let mut record_index = Vec::new();
        
        // Read n bytes for num record blocks
        let num_record_blocks = crate::read_int_from_filehandler(file_handler, offset, read_size);
        let num_entries = crate::read_int_from_filehandler(file_handler, offset, read_size);

        let byte_size_record_index = crate::read_int_from_filehandler(file_handler, offset, read_size);
        let byte_size_record_data = crate::read_int_from_filehandler(file_handler, offset, read_size);

        assert_eq!(num_record_blocks as usize * read_size * 2, byte_size_record_index as usize);
        
        let offset_before_index = *offset;

        // Create prefix sum of sizes
        record_index.push(RecordIndex {
            compressed_size: 0,
            uncompressed_size: 0,
        });
        for _ in 0..num_record_blocks {
            let compressed_size = crate::read_int_from_filehandler(file_handler, offset, read_size);
            let uncompressed_size = crate::read_int_from_filehandler(file_handler, offset, read_size);

            record_index.push(RecordIndex {
                compressed_size: record_index.last().unwrap().compressed_size + compressed_size,
                uncompressed_size: record_index.last().unwrap().uncompressed_size + uncompressed_size,
            });
        }

        assert_eq!(offset_before_index + byte_size_record_index, *offset);

        record_index
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::header::parser::HeaderInfo;
    use crate::key_index::parser::KeySection;
    use crate::file_reader::FileHandler;

    fn setup() -> (FileHandler, HeaderInfo, RecordSection) {
        let mut file_handler = FileHandler::open("resources/jitendex/jitendex.mdx").unwrap();
        let header_info = HeaderInfo::retrieve_header(&mut file_handler).unwrap();

        if !header_info.is_valid() {
            panic!("Invalid header");
        }

        let key_index = KeySection::retrieve_key_index(&mut file_handler, &header_info).unwrap();
        let record_section = RecordSection::parse(&header_info, &key_index, &mut file_handler);

        (file_handler, header_info, record_section)
    }

    #[test]
    fn test_record_section_parse() {
        let (mut file_handler, header_info, mut record_section) = setup();

        // Test Key ID: 280887285, Key Text: 飲
        let record_text = record_section.record_at_offset(280887285, &mut file_handler);
        
        assert_eq!(record_text, "@@@LINK=@jitendex-2799140");

        // Test cache functionality
        let record_text_cached = record_section.record_at_offset(280887285, &mut file_handler);
        assert_eq!(record_text_cached, "@@@LINK=@jitendex-2799140");
    }
}