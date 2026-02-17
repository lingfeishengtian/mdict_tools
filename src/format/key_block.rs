use std::io::{Cursor, Read};
use crate::error::Result;
use crate::types::KeyBlock;
use binrw::BinRead;

#[derive(BinRead)]
#[br(big)]
struct KeyId {
    key_id: u64,
}

fn read_nul_terminated_string(cur: &mut Cursor<&[u8]>, end: usize) -> Result<String> {
    let mut bytes = Vec::new();
    let mut one = [0u8; 1];

    while (cur.position() as usize) < end {
        cur.read_exact(&mut one)?;
        if one[0] == 0 { break; }
        bytes.push(one[0]);
    }

    Ok(String::from_utf8_lossy(&bytes).to_string())
}

/// Parse a decoded key block payload into `KeyBlock` entries.
pub fn parse_key_block(buf: &[u8]) -> Result<Vec<KeyBlock>> {
    let mut cur = Cursor::new(buf);
    let mut out = Vec::new();

    while (cur.position() as usize) < buf.len() {
        let key_id_struct = KeyId::read(&mut cur).map_err(|e| {
            crate::error::MDictError::from(format!("binrw error: {}", e))
        })?;

        let key_id = key_id_struct.key_id;

        let key_text = read_nul_terminated_string(&mut cur, buf.len())?;
        
        out.push(KeyBlock { key_id, key_text });
    }

    Ok(out)
}
