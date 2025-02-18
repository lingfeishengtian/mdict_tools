use std::{collections::HashMap, io};

use crate::{
    compressed_block::block::decode_block, file_reader::FileHandler, header::parser::{HeaderInfo, MdictVersion}, shared_macros::*
};

pub struct KeyIndex {
    section_offset: u64,
    key_info_offset: u64,
    next_section_offset: u64,
    key_info_blocks: Vec<KeyBlockInfo>,
    key_info_prefix_sum: Vec<u64>,
    num_blocks: u64,
    num_entries: u64,
    addler32_checksum: u32,
}

#[derive(Debug)]
pub struct KeyBlockInfo {
    num_entries: u64,
    first: String,
    last: String,
    compressed_size: u64,
    decompressed_size: u64,
}

#[derive(Debug)]
pub struct KeyBlock {
    key_id: u64,
    key_text: String,
}

impl KeyIndex {
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

        let num_blocks = Self::read_int_from_filehandler(file_handler, &mut offset, buf_size);
        let num_entries = Self::read_int_from_filehandler(file_handler, &mut offset, buf_size);
        let num_bytes_after_decomp_v2 = if header_info.get_version() == MdictVersion::V2 {
            Some(Self::read_int_from_filehandler(
                file_handler,
                &mut offset,
                buf_size,
            ))
        } else {
            None
        };
        let key_info_block_size =
            Self::read_int_from_filehandler(file_handler, &mut offset, buf_size);
        let key_blocks_size = Self::read_int_from_filehandler(file_handler, &mut offset, buf_size);

        // Addler32 checksum 4 bytes
        let addler32_checksum =
            Self::read_int_from_filehandler(file_handler, &mut offset, 4) as u32;
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

        Ok(KeyIndex {
            section_offset: header_info.size(),
            key_info_offset,
            next_section_offset: offset,
            key_info_blocks,
            num_blocks,
            num_entries,
            addler32_checksum,
            key_info_prefix_sum,
        })
    }

    pub fn next_section_offset(&self) -> u64 {
        self.next_section_offset
    }

    pub fn search_query(&self, query: &str, file_handler: &mut FileHandler) -> Option<Vec<KeyBlock>> {
        let (index, prefix_sum) = self.bin_search_key_info(query)?;

        // Set file handler to prefix_sum + section_offset
        let offset = self.key_info_offset + prefix_sum;
        let size = self.key_info_blocks[index as usize].compressed_size as usize;
        let mut buf = vec![0; size];
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
        }

        assert!(decoded_block.last().unwrap() == &0);

        Some(key_blocks)
    }

    fn bin_search_key_info(&self, query: &str) -> Option<(u64, u64)> {
        let mut start = 0;
        let mut end = self.num_blocks as usize - 1;

        while start <= end {
            let mid = start + (end - start) / 2;
            let key_info_block = &self.key_info_blocks[mid];

            if query < &key_info_block.first {
                end = mid - 1;
            } else if query > &key_info_block.last {
                start = mid + 1;
            } else {
                return Some((mid as u64, self.key_info_prefix_sum[mid]));
            }
        }

        None
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

    fn read_int_from_filehandler(
        file_handler: &mut FileHandler,
        offset: &mut u64,
        size: usize,
    ) -> u64 {
        let mut buf = vec![0; size];
        file_handler.read_from_file(*offset, &mut buf).unwrap();
        *offset += size as u64;

        match size {
            4 => read_int_from_buf_u32!(buf, 0) as u64,
            8 => read_int_from_buf_u64!(buf, 0),
            _ => panic!("Invalid buffer size"),
        }
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
            let size_of_first = read_int_from_buf!(buf, offset, size_of_first_or_last) + 1;
            // TODO: Detect encoding from header
            // let first_bytes = &buf[offset..offset + size_of_first as usize * 2];
            // let first = String::from_utf16(&first_bytes.chunks(2).map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]])).collect::<Vec<u16>>()).unwrap();
            let first =
                String::from_utf8(buf[offset..offset + size_of_first as usize].to_vec()).unwrap();
            offset += size_of_first as usize;

            // Add 1 for null terminator
            let size_of_last = read_int_from_buf!(buf, offset, size_of_first_or_last) + 1;
            // let last_bytes = &buf[offset..offset + size_of_last as usize * 2];
            // let last = String::from_utf16(&last_bytes.chunks(2).map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]])).collect::<Vec<u16>>()).unwrap();
            let last =
                String::from_utf8(buf[offset..offset + size_of_last as usize].to_vec()).unwrap();
            offset += size_of_last as usize;

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
    use super::*;

    fn setup() -> (FileHandler, HeaderInfo, KeyIndex) {
        let mut file_handler = FileHandler::open("resources/jitendex/jitendex.mdx").unwrap();
        let header_info = HeaderInfo::retrieve_header(&mut file_handler).unwrap();

        if !header_info.is_valid() {
            panic!("Invalid header");
        }

        let key_index = KeyIndex::retrieve_key_index(&mut file_handler, &header_info).unwrap();

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
        let _num_record_blocks = KeyIndex::read_int_from_filehandler(
            &mut file_handler,
            &mut next_section_offset,
            buf_size,
        );
        let num_entries = KeyIndex::read_int_from_filehandler(
            &mut file_handler,
            &mut next_section_offset,
            buf_size,
        );

        assert_eq!(num_entries, key_index.num_entries);
    }

    #[test]
    fn test_search_query() {
        // Track time to complete
        let start = std::time::Instant::now();
        let (mut file_handler, header_info, key_index) = setup();

        let query = "カエル";
        let key_blocks = key_index.search_query(query, &mut file_handler).unwrap();

        for key_block in key_blocks.iter() {
            if key_block.key_text.contains(query) {
                println!("Key ID: {}, Key Text: {}", key_block.key_id, key_block.key_text);
            }
        }

        println!("Time to complete: {:?}", start.elapsed());
    }
}
