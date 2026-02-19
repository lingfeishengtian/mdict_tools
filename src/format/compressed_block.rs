use std::io;
use binrw::{BinRead, BinReaderExt};
use crate::error::{Result, MDictError};

use zune_inflate::DeflateDecoder;
use minilzo_rs::{adler32, LZO};

/// Header-only representation for a compressed-format block.
#[derive(Debug, BinRead)]
#[br(little)]
pub struct CompressedBlockHeader {
    pub encoding: u32,
    #[br(big)]
    pub checksum: u32,
}

pub fn decode_format_block(buf: &[u8]) -> Result<Vec<u8>> {
    if buf.len() < 8 {
        return Err(MDictError::InvalidFormat("buffer too small".to_string()));
    }

    let mut cur = std::io::Cursor::new(buf);
    let fh: CompressedBlockHeader = CompressedBlockHeader::read(&mut cur)?;
    let encoding = fh.encoding;
    let expected_checksum = fh.checksum;
    let payload = &buf[8..];

    let res = match encoding {
        0 => payload.to_vec(),
        1 => {
            let lzo = LZO::init().map_err(|e| MDictError::InvalidFormat(format!("LZO init: {}", e)))?;
            lzo.decompress(payload, payload.len())
                .map_err(|e| MDictError::InvalidFormat(format!("LZO decompress: {}", e)))?
        }
        2 => {
            DeflateDecoder::new(payload)
                .decode_zlib()
                .map_err(|e| MDictError::InvalidFormat(format!("deflate decode: {}", e)))?
        }
        other => {
            return Err(MDictError::InvalidFormat(format!("unknown encoding: {}", other)));
        }
    };

    let checksum = adler32(&res);
    if checksum != expected_checksum {
        return Err(MDictError::InvalidFormat("invalid checksum".to_string()));
    }

    Ok(res)
}
