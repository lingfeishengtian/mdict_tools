use crate::error::Result;
use crate::types::{Encoding, KeyBlock};
use std::convert::TryInto;

fn read_nul_terminated(buf: &[u8], offset: &mut usize, encoding: Encoding) -> Result<String> {
    let rem = &buf[*offset..];

    match encoding {
        Encoding::Utf16LE => {
            let pos = rem
                .chunks_exact(2)
                .position(|c| c == [0, 0])
                .unwrap_or(rem.len() / 2);

            let bytes = &rem[..pos * 2];

            let s = String::from_utf16(
                &bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| crate::error::MDictError::from(format!("utf16 decode error: {e}")))?;

            *offset += pos * 2 + if pos * 2 < rem.len() { 2 } else { 0 };
            Ok(s)
        }

        _ => {
            let pos = rem.iter().position(|&b| b == 0).unwrap_or(rem.len());
            let s = String::from_utf8_lossy(&rem[..pos]).into_owned();
            *offset += pos + (pos < rem.len()) as usize;
            Ok(s)
        }
    }
}

fn read_key_id_be(buf: &[u8], offset: &mut usize) -> Result<u64> {
    let bytes = buf
        .get(*offset..*offset + 8)
        .ok_or_else(|| crate::error::MDictError::from("unexpected EOF while reading key_id"))?;

    *offset += 8;
    Ok(u64::from_be_bytes(bytes.try_into().unwrap()))
}

pub fn parse_key_block(buf: &[u8], encoding: Encoding) -> Result<Vec<KeyBlock>> {
    let mut offset = 0;
    let mut out = Vec::with_capacity(buf.len() / 16);

    while offset < buf.len() {
        out.push(KeyBlock {
            key_id: read_key_id_be(buf, &mut offset)?,
            key_text: read_nul_terminated(buf, &mut offset, encoding)?,
        });
    }

    Ok(out)
}
