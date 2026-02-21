use std::collections::HashMap;
use std::io::{Read, Seek};
use crate::error::Result;

use binrw::BinRead;
use xmlparser::{Tokenizer, Token};

fn unescape_xml(value: &str) -> String {
    value.replace("&quot;", "\"")
         .replace("&apos;", "'")
         .replace("&lt;", "<")
         .replace("&gt;", ">")
         .replace("&amp;", "&")
}

fn parse_attributes(xml: &str) -> HashMap<String, String> {
    let mut attributes = HashMap::new();
    let tokenizer = Tokenizer::from(xml);

    for token in tokenizer {
        if let Ok(token) = token {
            match token {
                Token::Attribute { prefix, local, value, .. } => {
                    let key = if prefix.is_empty() {
                        local.to_string()
                    } else {
                        format!("{}:{}", prefix, local)
                    };
                    attributes.insert(key, unescape_xml(value.as_str()));
                }
                _ => {}
            }
        }
    }

    attributes
}

#[derive(Debug, Clone)]
pub struct HeaderInfo {
    pub dict_info_size: u32,
    pub dict_info: HashMap<String, String>,
    pub adler32_checksum: u32,
}

#[derive(Debug, BinRead)]
#[br(big)]
struct HeaderRaw {
    dict_info_size: u32,
    #[br(count = dict_info_size as usize)]
    dict_info: Vec<u8>,
    adler32_checksum: u32,
}

impl HeaderInfo {
    /// Read header from a `Read + Seek` source using `binrw` for the fixed layout.
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let raw: HeaderRaw = HeaderRaw::read(reader)?;

        let buf16: Vec<u16> = raw.dict_info
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        let xml = String::from_utf16_lossy(&buf16);
        let dict_info = parse_attributes(&xml);

        Ok(HeaderInfo {
            dict_info_size: raw.dict_info_size,
            dict_info,
            adler32_checksum: raw.adler32_checksum,
        })
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.dict_info.get(key)
    }

    /// Return the declared encoding for dict info: `UTF-8` -> `Utf8`, otherwise default to `Utf16LE`.
    pub fn get_encoding(&self) -> crate::types::Encoding {
        if let Some(enc) = self.dict_info.get("Encoding") {
            if enc.eq_ignore_ascii_case("UTF-8") {
                crate::types::Encoding::Utf8
            } else {
                crate::types::Encoding::Utf16LE
            }
        } else {
            crate::types::Encoding::Utf16LE
        }
    }

    /// Return the engine version as an enum similar to the legacy parser.
    pub fn get_version(&self) -> crate::types::MdictVersion {
        if let Some(version) = self.dict_info.get("GeneratedByEngineVersion") {
            match version.as_str() {
                "1.0" => crate::types::MdictVersion::V1,
                "2.0" => crate::types::MdictVersion::V2,
                "3.0" => crate::types::MdictVersion::V3,
                _ => panic!("Unsupported version: {}", version),
            }
        } else {
            crate::types::MdictVersion::MDD
        }
    }

    /// Return header size in bytes (4 + dict_info_size + 4)
    pub fn size(&self) -> u64 {
        4 + self.dict_info_size as u64 + 4
    }
}
