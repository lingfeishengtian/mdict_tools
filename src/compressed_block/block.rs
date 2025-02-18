use std::io::{self, Read};
use zune_inflate::DeflateDecoder;
use minilzo_rs::{adler32, LZO};

use crate::shared_macros::*;

enum BlockEncoding {
    NoEncoding,
    Lzo,
    Gzip,
}

impl BlockEncoding {
    pub fn from_u32(value: u32) -> Option<BlockEncoding> {
        match value {
            0 => Some(BlockEncoding::NoEncoding),
            1 => Some(BlockEncoding::Lzo),
            2 => Some(BlockEncoding::Gzip),
            _ => None,
        }
    }
}

pub fn decode_block(block: &[u8]) -> io::Result<Vec<u8>> {
    let mut offset = 0;

    // Read first 4 bytes to get the encoding
    let encoding_int = read_int_from_buf_le!(block, offset, 4) as u32;

    let encoding = BlockEncoding::from_u32(encoding_int).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Invalid encoding")
    })?;
    
    let adler32_checksum = read_int_from_buf!(block, offset, 4) as u32;

    let res = match encoding {
        BlockEncoding::NoEncoding => {
            block[offset..].to_vec()
        }
        BlockEncoding::Lzo => {
            // TODO: Test this, since I don't have LZO compressed data to test
            let lzo = LZO::init().unwrap();
            lzo.decompress(&block[offset..], block.len() - offset).unwrap()
        }
        BlockEncoding::Gzip => {
            DeflateDecoder::new(&block[offset..]).decode_zlib().unwrap()
        }
    };

    // Check if the checksum is correct
    let checksum = adler32(&res);
    if checksum != adler32_checksum {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid checksum"));
    }

    Ok(res)
}