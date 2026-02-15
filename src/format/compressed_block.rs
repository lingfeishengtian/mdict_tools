use std::io;
use binrw::{BinRead, BinReaderExt};
use std::convert::TryInto;

use zune_inflate::DeflateDecoder;
use minilzo_rs::{adler32, LZO};

#[derive(Debug, BinRead)]
#[br(little)]
pub struct BlockHeader {
    pub encoding: u32,
    pub checksum: u32,
}

pub fn read_block_header(buf: &[u8]) -> Result<BlockHeader, binrw::Error> {
    let mut cursor = std::io::Cursor::new(buf);
    BlockHeader::read(&mut cursor)
}

pub fn decode_format_block(buf: &[u8]) -> io::Result<Vec<u8>> {
    if buf.len() < 8 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "buffer too small"));
    }

    let encoding = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    // Note: legacy code reads the checksum as big-endian (see shared_macros read_int_from_buf!),
    // so parse the checksum as BE to match behavior.
    let expected_checksum = u32::from_be_bytes(buf[4..8].try_into().unwrap());
    let payload = &buf[8..];

    let res = match encoding {
        0 => payload.to_vec(),
        1 => {
            // LZO
            let lzo = LZO::init().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("LZO init: {}", e)))?;
            lzo.decompress(payload, payload.len())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("LZO decompress: {}", e)))?
        }
        2 => {
            // Gzip/deflate wrapped in zlib stream
            DeflateDecoder::new(payload)
                .decode_zlib()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("deflate decode: {}", e)))?
        }
        other => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unknown encoding: {}", other)));
        }
    };

    let checksum = adler32(&res);
    if checksum != expected_checksum {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid checksum"));
    }

    Ok(res)
}
