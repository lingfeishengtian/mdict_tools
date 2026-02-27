use crate::error::{MDictError, Result};
use binrw::{BinRead, BinReaderExt};
use std::io;

use minilzo_rs::{adler32, LZO};
use zstd::bulk::decompress as zstd_decompress;
use zune_inflate::DeflateDecoder;

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
            let lzo =
                LZO::init().map_err(|e| MDictError::InvalidFormat(format!("LZO init: {}", e)))?;
            if payload.len() >= 4 {
                let expected_len =
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
                match lzo.decompress_safe(&payload[4..], expected_len) {
                    Ok(decoded) => decoded,
                    Err(_) => lzo
                        .decompress(payload, payload.len())
                        .map_err(|e| MDictError::InvalidFormat(format!("LZO decompress: {}", e)))?,
                }
            } else {
                lzo.decompress(payload, payload.len())
                    .map_err(|e| MDictError::InvalidFormat(format!("LZO decompress: {}", e)))?
            }
        }
        2 => DeflateDecoder::new(payload)
            .decode_zlib()
            .map_err(|e| MDictError::InvalidFormat(format!("deflate decode: {}", e)))?,
        4 => {
            if payload.len() < 4 {
                return Err(MDictError::InvalidFormat(
                    "zstd payload missing size prefix".to_string(),
                ));
            }
            let expected_len =
                u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
            zstd_decompress(&payload[4..], expected_len)
                .map_err(|e| MDictError::InvalidFormat(format!("zstd decode: {}", e)))?
        }
        other => {
            return Err(MDictError::InvalidFormat(format!(
                "unknown encoding: {}",
                other
            )));
        }
    };

    let checksum = adler32(&res);
    if checksum != expected_checksum {
        return Err(MDictError::InvalidFormat("invalid checksum".to_string()));
    }

    Ok(res)
}
