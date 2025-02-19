use std::{collections::HashMap, io};

use crate::{
    compressed_block::block::decode_block, file_reader::FileHandler, header::parser::{HeaderInfo, MdictVersion}, shared_macros::*
};

use super::search_result::{self, SearchResultPointer};

pub struct KeySection {
    section_offset: u64,
    key_info_offset: u64,
    next_section_offset: u64,
    key_info_blocks: Vec<KeyBlockInfo>,
    key_info_prefix_sum: Vec<u64>,
    num_blocks: u64,
    num_entries: u64,
    addler32_checksum: u32,
    cached_key_blocks: Option<(u64, Vec<KeyBlock>)>,
}

#[derive(Debug)]
pub struct KeyBlockInfo {
    pub num_entries: u64,
    first: String,
    last: String,
    compressed_size: u64,
    decompressed_size: u64,
}

impl KeyBlockInfo {
    pub fn contains(&self, query: &str) -> bool {
        self.first.starts_with(query) || self.last.starts_with(query) || (self.first.as_str() < query && self.last.as_str() > query)
    }

    pub fn is_less_than(&self, query: &str) -> bool {
        &self.last.as_str() < &query && !self.last.starts_with(query)
    }

    pub fn is_greater_than(&self, query: &str) -> bool {
        &self.first.as_str() > &query && !self.first.starts_with(query)
    }
}

#[derive(Debug)]
#[derive(Clone)]
pub struct KeyBlock {
    key_id: u64,
    key_text: String,
}

impl KeySection {
    pub fn retrieve_key_index(
        file_handler: &mut FileHandler,
        header_info: &HeaderInfo,
    ) -> io::Result<Self> {
        if header_info.get_version() == MdictVersion::V3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported version",
            ));
        }

        // Buffer
        let buf_size = match header_info.get_version() {
            MdictVersion::V1 => 4,
            MdictVersion::V2 => 8,
            MdictVersion::V3 => 0,
        };
        let mut offset = header_info.size();

        let num_blocks = crate::read_int_from_filehandler(file_handler, &mut offset, buf_size);
        let num_entries = crate::read_int_from_filehandler(file_handler, &mut offset, buf_size);
        let num_bytes_after_decomp_v2 = if header_info.get_version() == MdictVersion::V2 {
            Some(crate::read_int_from_filehandler(
                file_handler,
                &mut offset,
                buf_size,
            ))
        } else {
            None
        };
        let key_info_block_size =
            crate::read_int_from_filehandler(file_handler, &mut offset, buf_size);
        let key_blocks_size = crate::read_int_from_filehandler(file_handler, &mut offset, buf_size);

        // Addler32 checksum 4 bytes
        let addler32_checksum =
            crate::read_int_from_filehandler(file_handler, &mut offset, 4) as u32;
        let key_info_blocks = Self::read_key_info_block(
            file_handler,
            &mut offset,
            key_info_block_size as usize,
            num_bytes_after_decomp_v2,
        );

        // Add offset of key_info_blocks to offset
        let key_info_prefix_sum = Self::generate_key_info_prefix_sum(&key_info_blocks);
        let key_info_offset = offset;
        offset += key_info_prefix_sum.last().unwrap();

        Ok(KeySection {
            section_offset: header_info.size(),
            key_info_offset,
            next_section_offset: offset,
            key_info_blocks,
            key_info_prefix_sum,
            num_blocks,
            num_entries,
            addler32_checksum,
            cached_key_blocks: None,
        })
    }

    pub fn next_section_offset(&self) -> u64 {
        self.next_section_offset
    }

    fn decode_key_blocks(file_handler: &mut FileHandler, offset: u64, size: u64) -> Vec<KeyBlock> {
        let mut buf = vec![0; size as usize];
        file_handler.read_from_file(offset, &mut buf).unwrap();

        // Decode block
        let decoded_block = decode_block(&buf).unwrap();

        let mut key_blocks = Vec::new();
        let mut offset = 0;
        while offset < decoded_block.len() {
            let key_id = read_int_from_buf!(decoded_block, offset, 8);
            let key_text = String::from_utf8(
                decoded_block[offset..]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c)
                    .collect::<Vec<u8>>(),
            );

            offset += key_text.as_ref().unwrap().len() + 1;

            key_blocks.push(KeyBlock {
                key_id,
                key_text: key_text.unwrap(),
            });
        };

        key_blocks
    }

    pub fn key_index(&self, index: u64) -> &KeyBlockInfo {
        &self.key_info_blocks[index as usize]
    }

    pub fn read_block_index(&mut self, file_handler: &mut FileHandler, index: u64, key_section_offset: u64) -> KeyBlock {
        let key_info = &self.key_info_blocks[index as usize];
        let offset = self.key_info_offset + self.key_info_prefix_sum[index as usize];
        let size = key_info.compressed_size as usize;

        if let Some((cached_index, cached_key_blocks)) = &self.cached_key_blocks {
            if *cached_index == index {
                return cached_key_blocks[key_section_offset as usize].clone();
            }
        }

        println!("Cache miss index: {}", index);
        self.cached_key_blocks = Some((index, Self::decode_key_blocks(file_handler, offset, size as u64)));
        self.cached_key_blocks.as_ref().unwrap().1[key_section_offset as usize].clone()
    }

    pub fn search_query(&self, query: &str, file_handler: &mut FileHandler) -> Option<SearchResultPointer> {
        let (first, last) = self.get_encapsulating_indices(query)?;

        // Find first instance where prefix is the query in the first page
        // In the "first" page, find the first instance where the prefix is the query using read_block_index
        let start_ind = self.search_index_page_for_query_start_ind(query, file_handler, first as u64);

        // Find last instance where prefix is the query in the last page
        // In the "last" page, find the last instance where the prefix is the query using read_block_index
        let end_ind = self.search_index_page_for_query_end_ind(query, file_handler, last as u64);

        Some(SearchResultPointer::new(first, last, start_ind, end_ind))
    }

    fn search_index_page_for_query_start_ind(&self, query: &str, file_handler: &mut FileHandler, index: u64) -> u64 {
        let key_info = &self.key_info_blocks[index as usize];
        let offset = self.key_info_offset + self.key_info_prefix_sum[index as usize];
        let size = key_info.compressed_size as usize;

        let key_blocks = Self::decode_key_blocks(file_handler, offset, size as u64);

        let mut start = 0;
        let mut end = key_info.num_entries;
        let mut result = None;

        while start <= end {
            let mid = start + (end - start) / 2;
            let key_block = &key_blocks[mid as usize];

            if (mid == 0 || key_blocks[mid as usize - 1].key_text.as_str() < query) && key_block.key_text.starts_with(query) {
                return mid;
            } else if key_block.key_text.as_str() < query && !key_block.key_text.starts_with(query) {
                // Search the right half
                start = mid + 1;
            } else {
                // Search the left half
                end = mid - 1;
            }
        }

        result.unwrap()
    }

    fn search_index_page_for_query_end_ind(&self, query: &str, file_handler: &mut FileHandler, index: u64) -> u64 {
        let key_info = &self.key_info_blocks[index as usize];
        let offset = self.key_info_offset + self.key_info_prefix_sum[index as usize];
        let size = key_info.compressed_size as usize;

        let key_blocks = Self::decode_key_blocks(file_handler, offset, size as u64);

        let mut start = 0;
        let mut end = key_info.num_entries;
        let mut result = None;

        while start <= end {
            let mid = start + (end - start) / 2;
            let key_block = &key_blocks[mid as usize];

            if (mid == key_info.num_entries - 1 || (key_blocks[mid as usize + 1].key_text.as_str() > query && !key_blocks[mid as usize + 1].key_text.starts_with(query))) && key_block.key_text.starts_with(query) {
                return mid + 1;
            } else if key_block.key_text.as_str() > query && !key_block.key_text.starts_with(query) {
                // Search the left half
                end = mid - 1;
            } else {
                // Search the right half
                start = mid + 1;
            }
        }

        result.unwrap()
    }

    fn find_first_block_starting_with(&self, query: &str) -> Option<usize> {
        let mut start = 0;
        let mut end = self.num_blocks as usize - 1;
        let mut result = None;
    
        while start <= end {
            let mid = start + (end - start) / 2;
            let key_info = &self.key_info_blocks[mid];
    
            if (mid == 0 || self.key_info_blocks[mid - 1].is_less_than(query)) && key_info.contains(query) {
                return Some(mid);
            } else if key_info.is_less_than(query) {
                // Search the right half
                start = mid + 1;
            } else {
                // Search the left half
                end = mid - 1;
            }
        }
    
        result
    }
    
    fn find_last_block_starting_with(&self, query: &str) -> Option<usize> {
        let mut start = 0;
        let mut end = self.num_blocks as usize - 1;
        let mut result = None;
    
        while start <= end {
            let mid = start + (end - start) / 2;
            let key_info = &self.key_info_blocks[mid];
    
            if (mid == self.num_blocks as usize - 1 || self.key_info_blocks[mid + 1].is_greater_than(query))
                && key_info.contains(query)
            {
                return Some(mid);
            } else if key_info.is_greater_than(query) {
                // Search the left half
                end = mid - 1;
            } else {
                // Search the right half
                start = mid + 1;
            }
        }
    
        result
    }
    
    fn get_encapsulating_indices(&self, query: &str) -> Option<(usize, usize)> {
        let first_index = self.find_first_block_starting_with(query);
        let last_index = self.find_last_block_starting_with(query);
    
        match (first_index, last_index) {
            (Some(first), Some(last)) => Some((first, last)),
            _ => None,
        }
    }

    fn generate_key_info_prefix_sum(key_info_blocks: &Vec<KeyBlockInfo>) -> Vec<u64> {
        let mut prefix_sum = Vec::new();
        let mut sum = 0;

        // Push 0 indicating the base case
        prefix_sum.push(sum);
        for key_info_block in key_info_blocks.iter() {
            sum += key_info_block.compressed_size;
            prefix_sum.push(sum);
        }

        prefix_sum
    }

    fn read_key_info_block(
        file_handler: &mut FileHandler,
        offset: &mut u64,
        size_of_key_info_block: usize,
        size_after_decomp_v2: Option<u64>,
    ) -> Vec<KeyBlockInfo> {
        let mut buf = vec![0; size_of_key_info_block];
        file_handler.read_from_file(*offset, &mut buf).unwrap();
        *offset += size_of_key_info_block as u64;

        if let Some(size_after_decomp_v2) = size_after_decomp_v2 {
            buf = decode_block(&buf).unwrap();
            // Ensure size of decompressed buffer is equal to the size after decompression
            assert_eq!(buf.len() as u64, size_after_decomp_v2);
        }

        let mut key_info_vector = Vec::new();
        let mut offset = 0;

        // Size of first or last depends on the version, so if it's V1, it's 1 bytes, otherwise 2 bytes
        let size_of_first_or_last = match size_after_decomp_v2 {
            Some(_) => 2,
            None => 1,
        };

        while offset < buf.len() {
            // TODO: Figure out whether the number of bytes derive from the version
            let num_entries = read_int_from_buf!(buf, offset, 8);

            // Add 1 for null terminator
            let size_of_first = read_int_from_buf!(buf, offset, size_of_first_or_last);
            // TODO: Detect encoding from header
            // let first_bytes = &buf[offset..offset + size_of_first as usize * 2];
            // let first = String::from_utf16(&first_bytes.chunks(2).map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]])).collect::<Vec<u16>>()).unwrap();
            let first =
                String::from_utf8(buf[offset..offset + size_of_first as usize].to_vec()).unwrap();
            offset += size_of_first as usize + 1;

            // Add 1 for null terminator
            let size_of_last = read_int_from_buf!(buf, offset, size_of_first_or_last);
            // let last_bytes = &buf[offset..offset + size_of_last as usize * 2];
            // let last = String::from_utf16(&last_bytes.chunks(2).map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]])).collect::<Vec<u16>>()).unwrap();
            let last =
                String::from_utf8(buf[offset..offset + size_of_last as usize].to_vec()).unwrap();
            offset += size_of_last as usize + 1;

            let compressed_size = read_int_from_buf!(buf, offset, 8);
            let decompressed_size = read_int_from_buf!(buf, offset, 8);

            key_info_vector.push(KeyBlockInfo {
                num_entries,
                first,
                last,
                compressed_size,
                decompressed_size,
            });
        }

        key_info_vector
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::key_index;

    use super::*;
    use get_size2::GetSize;

    fn setup() -> (FileHandler, HeaderInfo, KeySection) {
        let mut file_handler = FileHandler::open("resources/jitendex/jitendex.mdx").unwrap();
        let header_info = HeaderInfo::retrieve_header(&mut file_handler).unwrap();

        if !header_info.is_valid() {
            panic!("Invalid header");
        }

        let key_index = KeySection::retrieve_key_index(&mut file_handler, &header_info).unwrap();

        (file_handler, header_info, key_index)
    }

    #[test]
    fn test_valid_key_section() {
        let (mut file_handler, header_info, key_index) = setup();

        assert_eq!(key_index.num_blocks, key_index.key_info_blocks.len() as u64);

        let mut total_entries = 0;
        for key_block_info in key_index.key_info_blocks.iter() {
            total_entries += key_block_info.num_entries;
        }

        assert_eq!(total_entries, key_index.num_entries);

        // Validate that we enter the next section
        let mut next_section_offset = key_index.next_section_offset();

        let buf_size = match header_info.get_version() {
            MdictVersion::V1 => 4,
            MdictVersion::V2 => 8,
            MdictVersion::V3 => 0,
        };
        let _num_record_blocks = crate::read_int_from_filehandler(
            &mut file_handler,
            &mut next_section_offset,
            buf_size,
        );
        let num_entries = crate::read_int_from_filehandler(
            &mut file_handler,
            &mut next_section_offset,
            buf_size,
        );
        
        assert_eq!(num_entries, key_index.num_entries);
    }

    #[test]
    fn test_bin_search() {
        let (mut file_handler, header_info, key_index) = setup();

        for key_info in key_index.key_info_blocks.iter() {
            println!("First: {}, Last: {}", key_info.first, key_info.last);
        }

        let query = "つ";
        let (first, last) = retrieve_query_encapsulating_indices(&key_index, query);

        assert_eq!(first, "ちんがいやく");
        assert_eq!(last, "ていし");

        let query = "か";
        let (first, last) = retrieve_query_encapsulating_indices(&key_index, query);

        assert_eq!(first, "おんなやもめ【女寡】");
        assert_eq!(last, "がいこくまい【外国米】");

        let query = "付";
        let (first, last) = retrieve_query_encapsulating_indices(&key_index, query);

        assert_eq!(first, "人智を越えた");
        assert_eq!(last, "伴僧");
    }

    fn retrieve_query_encapsulating_indices(key_index: &KeySection, query: &str) -> (String, String) {
        let (first_ind, last_ind) = key_index.get_encapsulating_indices(query).unwrap();
        let first = key_index.key_info_blocks[first_ind].first.clone();
        let last = key_index.key_info_blocks[last_ind].last.clone();
        (first, last)
    }
    
    #[test]
    fn test_search_query() {
        // Track time to complete
        let (mut file_handler, header_info, mut key_index) = setup();

        // TODO: Handle underflow
        // let query = "@jitendex-1000000";
        let query = "食う";

        let start = std::time::Instant::now();
        // key_index.cache_key_blocks(&mut file_handler);
        let mut key_blocks = key_index.search_query(query, &mut file_handler).unwrap();
        println!("Time to complete: {:?}", start.elapsed());

        // First should be 食う second should be 食うや食わず
        let first = key_blocks.next(&mut file_handler, &mut key_index).unwrap();
        let second = key_blocks.next(&mut file_handler, &mut key_index).unwrap();

        assert_eq!(first.key_text, "食う");
        assert_eq!(second.key_text, "食うや食わず");

        assert_eq!(0, key_index.get_heap_size());
    }

    impl GetSize for KeyBlock {
        fn get_size(&self) -> usize {
            std::mem::size_of_val(&self.key_id) + self.key_text.get_heap_size()
        }
    }

    impl GetSize for KeyBlockInfo {
        fn get_size(&self) -> usize {
            std::mem::size_of_val(&self.num_entries) + self.first.get_heap_size() + self.last.get_heap_size() + std::mem::size_of_val(&self.compressed_size) + std::mem::size_of_val(&self.decompressed_size)
        }
    }

    impl GetSize for KeySection {
        fn get_size(&self) -> usize {
            std::mem::size_of_val(&self.section_offset) + std::mem::size_of_val(&self.key_info_offset) + std::mem::size_of_val(&self.next_section_offset) + self.key_info_blocks.get_heap_size() + self.key_info_prefix_sum.get_heap_size() + std::mem::size_of_val(&self.num_blocks) + std::mem::size_of_val(&self.num_entries) + std::mem::size_of_val(&self.addler32_checksum)
        }
    }
}
