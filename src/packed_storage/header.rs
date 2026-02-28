use std::io::{Cursor, Read, Seek, Write};

use binrw::{BinRead, BinWrite};

use crate::error::{MDictError, Result};

use super::CompressionEncoding;

pub const MAGIC: [u8; 8] = *b"PKGSTRG1";
pub const VERSION: u8 = 1;

const FIXED_HEADER_SIZE: usize = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(BinRead, BinWrite)]
#[brw(little)]
pub struct BlockPrefixEntry {
    pub compressed_end: u64,
    pub uncompressed_end: u64,
}

#[derive(Debug, Clone, BinRead, BinWrite)]
#[brw(little)]
#[br(assert(version == VERSION, "unsupported packed storage version"))]
#[bw(assert(
    *reserved_flags == 0 && *reserved_flags_padding == 0,
    "reserved header flags are not zero"
))]
#[bw(assert(
    *reserved_encoding_padding == 0,
    "reserved header padding is not zero"
))]
#[bw(assert(*num_blocks > 0, "packed storage requires at least one prefix entry"))]
#[br(assert(
    num_blocks as usize == block_prefix_sum.len(),
    "num_blocks does not match prefix entries"
))]
#[brw(assert(
    block_prefix_sum
        .first()
        .map(|entry| entry.compressed_end == 0 && entry.uncompressed_end == 0)
        .unwrap_or(false),
    "first prefix entry must be (0, 0)"
))]
#[brw(assert(
    block_prefix_sum.windows(2).all(|window| {
        let prev = window[0];
        let current = window[1];
        current.compressed_end >= prev.compressed_end
            && current.uncompressed_end >= prev.uncompressed_end
    }),
    "prefix entries must be monotonic"
))]
struct PackedStorageHeaderRaw {
    #[brw(magic(b"PKGSTRG1"))]
    version: u8,
    reserved_flags: u8,
    reserved_flags_padding: u16,
    encoding: u8,
    compression_level: u8,
    reserved_encoding_padding: u16,
    num_blocks: u64,
    num_entries: u64,
    #[br(count = num_blocks as usize)]
    block_prefix_sum: Vec<BlockPrefixEntry>,
}

#[derive(Debug, Clone)]
pub struct PackedStorageHeader {
    pub encoding: CompressionEncoding,
    pub compression_level: u8,
    pub num_entries: u64,
    pub block_prefix_sum: Vec<BlockPrefixEntry>,
}

impl PackedStorageHeader {
    pub fn encoded_len(&self) -> Result<usize> {
        let prefix_bytes = self
            .block_prefix_sum
            .len()
            .checked_mul(16)
            .ok_or_else(|| MDictError::InvalidFormat("header size overflow".to_string()))?;
        FIXED_HEADER_SIZE
            .checked_add(prefix_bytes)
            .ok_or_else(|| MDictError::InvalidFormat("header size overflow".to_string()))
    }

    pub fn write_to_bytes(&self) -> Result<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::with_capacity(self.encoded_len()?));
        self.write_to_writer(&mut cursor)?;
        Ok(cursor.into_inner())
    }

    pub fn write_to_writer<W: Write + Seek>(&self, writer: &mut W) -> Result<()> {
        let num_blocks = u64::try_from(self.block_prefix_sum.len())
            .map_err(|_| MDictError::InvalidFormat("num_blocks overflow".to_string()))?;

        let raw = PackedStorageHeaderRaw {
            version: VERSION,
            reserved_flags: 0,
            reserved_flags_padding: 0,
            encoding: self.encoding.as_u8(),
            compression_level: self.compression_level,
            reserved_encoding_padding: 0,
            num_blocks,
            num_entries: self.num_entries,
            block_prefix_sum: self.block_prefix_sum.clone(),
        };

        raw.write_le(writer)?;
        Ok(())
    }

    pub fn parse_from_bytes(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < FIXED_HEADER_SIZE {
            return Err(MDictError::InvalidFormat(
                "packed storage file too small for fixed header".to_string(),
            ));
        }

        let mut reader = Cursor::new(data);
        let (header, data_offset) = Self::parse_from_reader(&mut reader)?;
        if data_offset > data.len() {
            return Err(MDictError::InvalidFormat(
                "prefix table exceeds file size".to_string(),
            ));
        }

        Ok((header, data_offset))
    }

    pub fn parse_from_reader<R: Read + Seek>(reader: &mut R) -> Result<(Self, usize)> {
        let raw = PackedStorageHeaderRaw::read_le(reader)?;
        let encoding = CompressionEncoding::from_u8(raw.encoding)?;

        let num_blocks = usize::try_from(raw.num_blocks)
            .map_err(|_| MDictError::InvalidFormat("num_blocks overflow".to_string()))?;
        let prefix_bytes = num_blocks
            .checked_mul(16)
            .ok_or_else(|| MDictError::InvalidFormat("prefix table size overflow".to_string()))?;
        let data_offset = FIXED_HEADER_SIZE
            .checked_add(prefix_bytes)
            .ok_or_else(|| {
                MDictError::InvalidFormat("packed storage header size overflow".to_string())
            })?;

        Ok((
            PackedStorageHeader {
                encoding,
                compression_level: raw.compression_level,
                num_entries: raw.num_entries,
                block_prefix_sum: raw.block_prefix_sum,
            },
            data_offset,
        ))
    }
}
