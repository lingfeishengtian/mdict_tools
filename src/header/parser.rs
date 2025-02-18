use std::{collections::HashMap, io};

use crate::file_reader::{FileHandler};

#[derive(Debug)]
pub struct HeaderInfo {
    dict_info_size: u32,
    dict_info: HashMap<String, String>,
    adler32_checksum: u32,
}

#[derive(Debug)]
#[derive(PartialEq)]
pub enum MdictVersion {
    V1,
    V2,
    V3,
}

const REQUIRED_DICT_INFO_KEYS: [&str; 3] = ["RequiredEngineVersion", "Encoding", "Encrypted"];

impl HeaderInfo {
    pub fn retrieve_header(file_handler: &mut FileHandler) -> io::Result<Self> {
        // First read 4 bytes to get the size of the dictionary info string
        let mut buf = [0; 4];
        file_handler.read_from_file(0, &mut buf)?;
        let dict_info_size = u32::from_be_bytes(buf);

        // Read the dictionary info string
        let dict_info = file_handler.read_parse_xml(4, dict_info_size as u64)?;

        // Read the adler32 checksum
        let mut buf = [0; 4];
        file_handler.read_from_file(4 + dict_info_size as u64, &mut buf)?;

        Ok(HeaderInfo {
            dict_info_size,
            dict_info,
            adler32_checksum: u32::from_be_bytes(buf),
        })
    }

    pub fn dict_info(&self) -> &HashMap<String, String> {
        &self.dict_info
    }

    pub fn adler32_checksum(&self) -> u32 {
        self.adler32_checksum
    }

    pub fn is_valid(&self) -> bool {
        for key in REQUIRED_DICT_INFO_KEYS.iter() {
            if !self.dict_info.contains_key(*key) {
                return false;
            }
        }

        if self.dict_info.get("Encrypted").unwrap() != "No" {
            return false;
        }

        true
    }

    pub fn get_version(&self) -> MdictVersion {
        let version = self.dict_info.get("GeneratedByEngineVersion").unwrap();
        match version.as_str() {
            "1.0" => MdictVersion::V1,
            "2.0" => MdictVersion::V2,
            "3.0" => MdictVersion::V3,
            _ => panic!("Unsupported version: {}", version),
        }
    }

    pub fn size(&self) -> u64 {
        4 + self.dict_info_size as u64 + 4
    }
}