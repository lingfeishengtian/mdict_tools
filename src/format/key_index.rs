use std::io::{Read, Seek};
use crate::error::Result;
use crate::format::HeaderInfo;
use crate::compressed_block::block::decode_block;
use binrw::BinRead;

#[derive(Debug, Clone)]
pub struct KeyBlockInfo {
    pub num_entries: u64,
    pub first: String,
    pub last: String,
    pub compressed_size: u64,
    pub decompressed_size: u64,
}

#[derive(Debug)]
pub struct KeySection {
    pub section_offset: u64,
    pub key_info_offset: u64,
    pub next_section_offset: u64,
    pub key_info_blocks: Vec<KeyBlockInfo>,
    pub key_info_prefix_sum: Vec<u64>,
    pub num_blocks: u64,
    pub num_entries: u64,
    pub addler32_checksum: u32,
}

// Outer header raw structs for binrw parsing
#[derive(Debug, BinRead)]
#[br(big)]
struct KeySectionV1Raw {
    num_blocks: u32,
    num_entries: u32,
    key_info_block_size: u32,
    key_blocks_size: u32,
    addler32_checksum: u32,
    #[br(count = key_info_block_size as usize)]
    key_info: Vec<u8>,
}

#[derive(Debug, BinRead)]
#[br(big)]
struct KeySectionV2Raw {
    num_blocks: u64,
    num_entries: u64,
    num_bytes_after_decomp_v2: u64,
    key_info_block_size: u64,
    key_blocks_size: u64,
    addler32_checksum: u32,
    #[br(count = key_info_block_size as usize)]
    key_info: Vec<u8>,
}

impl KeySection {
    pub fn read_from<R: Read + Seek>(reader: &mut R, header: &HeaderInfo) -> Result<Self> {
        // Seek to header end
        reader.seek(std::io::SeekFrom::Start(header.size()))?;

        let version = match header.get_version() {
            crate::header::parser::MdictVersion::V1 => 1u8,
            crate::header::parser::MdictVersion::V2 => 2u8,
            crate::header::parser::MdictVersion::V3 => return Err("Unsupported version".into()),
        };

        // Parse the outer header using binrw depending on version
        let (num_blocks, num_entries, num_bytes_after_decomp_v2, key_info_block_size, key_blocks_size, addler32_checksum, mut key_info_buf) =
            if version == 1 {
                let raw: KeySectionV1Raw = binrw::BinRead::read_be(reader)?;
                (
                    raw.num_blocks as u64,
                    raw.num_entries as u64,
                    None,
                    raw.key_info_block_size as u64,
                    raw.key_blocks_size as u64,
                    raw.addler32_checksum,
                    raw.key_info,
                )
            } else {
                let raw: KeySectionV2Raw = binrw::BinRead::read_be(reader)?;
                (
                    raw.num_blocks,
                    raw.num_entries,
                    Some(raw.num_bytes_after_decomp_v2),
                    raw.key_info_block_size,
                    raw.key_blocks_size,
                    raw.addler32_checksum,
                    raw.key_info,
                )
            };

        // Capture key_info_offset: current position minus the key_info buffer length
        let key_info_offset = reader.seek(std::io::SeekFrom::Current(0))? - key_info_block_size;

        // Possibly decompress key_info
        if let Some(size_after) = num_bytes_after_decomp_v2 {
            let decompressed = decode_block(&key_info_buf)?;
            assert_eq!(decompressed.len() as u64, size_after);
            key_info_buf = decompressed;
        }

        // Parse key_info_buf into KeyBlockInfo entries using manual byte-slice parsing
        let mut offset: usize = 0;
        let buf_len = key_info_buf.len();
        let size_of_first_or_last = if num_bytes_after_decomp_v2.is_some() { 2usize } else { 1usize };
        let mut key_info_blocks = Vec::new();

        while offset < buf_len {
            // num_entries (u64, big-endian)
            if offset + 8 > buf_len { return Err("truncated key_info num_entries".into()); }
            let num_entries_field = u64::from_be_bytes(key_info_buf[offset..offset+8].try_into().unwrap());
            offset += 8;

            // size_of_first
            let size_of_first = if size_of_first_or_last == 1 {
                if offset + 1 > buf_len { return Err("truncated size_of_first".into()); }
                let v = key_info_buf[offset] as usize; offset += 1; v
            } else {
                if offset + 2 > buf_len { return Err("truncated size_of_first".into()); }
                let v = u16::from_be_bytes(key_info_buf[offset..offset+2].try_into().unwrap()) as usize; offset += 2; v
            };

            if offset + size_of_first > buf_len { return Err("truncated first bytes".into()); }
            let first_bytes = &key_info_buf[offset..offset + size_of_first]; offset += size_of_first;
            // consume null terminator
            if offset >= buf_len { return Err("missing null after first".into()); }
            offset += 1;
            let first = String::from_utf8(first_bytes.to_vec()).map_err(|_| "invalid utf8 in first" )?;

            // size_of_last
            let size_of_last = if size_of_first_or_last == 1 {
                if offset + 1 > buf_len { return Err("truncated size_of_last".into()); }
                let v = key_info_buf[offset] as usize; offset += 1; v
            } else {
                if offset + 2 > buf_len { return Err("truncated size_of_last".into()); }
                let v = u16::from_be_bytes(key_info_buf[offset..offset+2].try_into().unwrap()) as usize; offset += 2; v
            };

            if offset + size_of_last > buf_len { return Err("truncated last bytes".into()); }
            let last_bytes = &key_info_buf[offset..offset + size_of_last]; offset += size_of_last;
            if offset >= buf_len { return Err("missing null after last".into()); }
            offset += 1;
            let last = String::from_utf8(last_bytes.to_vec()).map_err(|_| "invalid utf8 in last" )?;

            // compressed_size, decompressed_size
            if offset + 8 > buf_len { return Err("truncated compressed_size".into()); }
            let compressed_size = u64::from_be_bytes(key_info_buf[offset..offset+8].try_into().unwrap()); offset += 8;
            if offset + 8 > buf_len { return Err("truncated decompressed_size".into()); }
            let decompressed_size = u64::from_be_bytes(key_info_buf[offset..offset+8].try_into().unwrap()); offset += 8;

            key_info_blocks.push(KeyBlockInfo {
                num_entries: num_entries_field,
                first,
                last,
                compressed_size,
                decompressed_size,
            });
        }

        // Build prefix sum
        let mut prefix_sum = Vec::with_capacity(key_info_blocks.len() + 1);
        prefix_sum.push(0u64);
        let mut sum = 0u64;
        for kb in &key_info_blocks {
            sum += kb.compressed_size;
            prefix_sum.push(sum);
        }
        // Compute next_section_offset: after key_info block and the following key_blocks area
        let next_section_offset = key_info_offset + key_info_block_size + key_blocks_size;

        Ok(KeySection {
            section_offset: header.size(),
            key_info_offset,
            next_section_offset,
            key_info_blocks,
            key_info_prefix_sum: prefix_sum,
            num_blocks,
            num_entries,
            addler32_checksum,
        })
    }
}
