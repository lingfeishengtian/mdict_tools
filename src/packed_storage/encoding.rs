use crate::error::{MDictError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressionEncoding {
    Raw = 0,
    Lzo = 1,
    Gzip = 2,
    Zstd = 3,
    Lz4 = 4,
}

impl CompressionEncoding {
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::Lzo),
            2 => Ok(Self::Gzip),
            3 => Ok(Self::Zstd),
            4 => Ok(Self::Lz4),
            _ => Err(MDictError::InvalidFormat(format!(
                "unsupported compression encoding id: {}",
                value
            ))),
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

pub fn encode_block(
    encoding: CompressionEncoding,
    compression_level: u8,
    data: &[u8],
) -> Result<Vec<u8>> {
    match encoding {
        CompressionEncoding::Raw => Ok(data.to_vec()),
        CompressionEncoding::Zstd => {
            let mapped_level = if compression_level == 0 {
                10
            } else {
                compression_level.min(10) as i32
            };
            zstd::bulk::compress(data, mapped_level)
                .map_err(|e| MDictError::InvalidFormat(e.to_string()))
        }
        CompressionEncoding::Lzo | CompressionEncoding::Gzip | CompressionEncoding::Lz4 => {
            Err(MDictError::UnsupportedFeature(format!(
                "encoder not implemented for {:?}",
                encoding
            )))
        }
    }
}

pub fn decode_block(
    encoding: CompressionEncoding,
    compressed: &[u8],
    expected_uncompressed_size: usize,
) -> Result<Vec<u8>> {
    match encoding {
        CompressionEncoding::Raw => Ok(compressed.to_vec()),
        CompressionEncoding::Zstd => zstd::bulk::decompress(compressed, expected_uncompressed_size)
            .map_err(|e| MDictError::InvalidFormat(e.to_string())),
        CompressionEncoding::Lzo | CompressionEncoding::Gzip | CompressionEncoding::Lz4 => {
            Err(MDictError::UnsupportedFeature(format!(
                "decoder not implemented for {:?}",
                encoding
            )))
        }
    }
}
