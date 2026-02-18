use std::io::{Read, Seek};
use crate::error::Result;
use crate::format::HeaderInfo;
use crate::format::decode_format_block as decode_block;
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
    #[br(calc = 0)] // V1 does not have this field, so set to 0
    num_bytes_after_decomp_v2: u32,
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

// Per-block raw structs for binrw parsing (two variants: v1 uses u8 sizes, v2 uses u16)
#[binrw::binread]
#[br(big)]
#[br(import(char_width: usize))]
#[derive(Debug)]
struct KeyBlockInfoV1Raw {
    num_entries: u64,

    #[br(temp)]
    size_of_first: u8,
    #[br(count = size_of_first as usize * char_width)]
    first: Vec<u8>,
    #[br(temp, count = char_width)]
    first_null: Vec<u8>,

    #[br(temp)]
    size_of_last: u8,
    #[br(count = size_of_last as usize * char_width)]
    last: Vec<u8>,
    #[br(temp, count = char_width)]
    last_null: Vec<u8>,

    compressed_size: u64,
    decompressed_size: u64,
}

#[binrw::binread]
#[br(big)]
#[br(import(char_width: usize))]
#[derive(Debug)]
struct KeyBlockInfoV2Raw {
    num_entries: u64,

    #[br(temp)]
    size_of_first: u16,
    #[br(count = size_of_first as usize * char_width)]
    first: Vec<u8>,
    #[br(temp, count = char_width)]
    first_null: Vec<u8>,

    #[br(temp)]
    size_of_last: u16,
    #[br(count = size_of_last as usize * char_width)]
    last: Vec<u8>,
    #[br(temp, count = char_width)]
    last_null: Vec<u8>,

    compressed_size: u64,
    decompressed_size: u64,
}

impl KeySection {
    pub fn read_from<R: Read + Seek>(reader: &mut R, header: &HeaderInfo) -> Result<Self> {
        reader.seek(std::io::SeekFrom::Start(header.size()))?;

        let ver = header.get_version();
        let (num_blocks, num_entries, num_bytes_after_decomp_v2, key_info_block_size, key_blocks_size, addler32_checksum, mut key_info_buf) =
            versioned_read!(ver, reader,
                v1: KeySectionV1Raw,
                v2: KeySectionV2Raw,
                as raw => {
                    (
                        raw.num_blocks as u64,
                        raw.num_entries as u64,
                        if ver.major() >= 2 { Some(raw.num_bytes_after_decomp_v2 as u64) } else { None },
                        raw.key_info_block_size as u64,
                        raw.key_blocks_size as u64,
                        raw.addler32_checksum,
                        raw.key_info,
                    )
                }
            );

        let key_info_offset = reader.seek(std::io::SeekFrom::Current(0))? - key_info_block_size;

        if let Some(size_after) = num_bytes_after_decomp_v2 {
            let decompressed = decode_block(&key_info_buf)?;
            assert_eq!(decompressed.len() as u64, size_after);
            key_info_buf = decompressed;
        }

        let size_of_first_or_last = header.get_encoding().char_width();
        let key_info_blocks = parse_key_info_binrw(ver, &key_info_buf, size_of_first_or_last)?;

        // Build prefix sum
        let mut prefix_sum = Vec::with_capacity(key_info_blocks.len() + 1);
        prefix_sum.push(0u64);
        let mut sum = 0u64;
        for kb in &key_info_blocks {
            sum += kb.compressed_size;
            prefix_sum.push(sum);
        }
        // Compute next_section_offset
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

fn parse_key_info_binrw(ver: crate::types::MdictVersion, buf: &[u8], size_of_first_or_last: usize) -> Result<Vec<KeyBlockInfo>> {
    use std::io::Cursor;

    let mut cur = Cursor::new(buf);
    let mut out = Vec::new();

    while (cur.position() as usize) < buf.len() {
        versioned_read_args!(
            ver, &mut cur, import: (size_of_first_or_last,),
            v1: KeyBlockInfoV1Raw,
            v2: KeyBlockInfoV2Raw,
            as raw => {
                let first = decode_key_text(raw.first, size_of_first_or_last)?;
                let last = decode_key_text(raw.last, size_of_first_or_last)?;

                out.push(KeyBlockInfo {
                    num_entries: raw.num_entries,
                    first,
                    last,
                    compressed_size: raw.compressed_size,
                    decompressed_size: raw.decompressed_size,
                });
            }
        );
    }

    Ok(out)
}

// Helper to decode key text bytes into a `String` depending on `char_width`.
// `char_width == 1` => UTF-8 single-byte; otherwise UTF-16LE (2-byte units).
fn decode_key_text(buf: Vec<u8>, char_width: usize) -> Result<String> {
    if char_width == 1 {
        String::from_utf8(buf).map_err(|_| format!("invalid utf8").into())
    } else {
        if buf.len() % 2 != 0 {
            return Err(format!("invalid utf16 length").into());
        }
        let u16_iter = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]));
        let s = std::char::decode_utf16(u16_iter)
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect::<String>();
        Ok(s)
    }
}