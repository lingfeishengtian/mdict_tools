use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek};
use super::xml_parser::parse_single_xml;

pub struct FileHandler {
    file: File,
    current_location: u64,
}

impl FileHandler {
    pub fn open(file_path: &str) -> io::Result<Self> {
        let file = File::open(file_path)?;
        Ok(FileHandler { file, current_location: 0 })
    }

    fn set_file_location(&mut self, location: u64) -> io::Result<u64> {
        self.file.seek(io::SeekFrom::Start(location))
    }

    pub fn read_from_file(&mut self, location: u64, buf: &mut [u8]) -> io::Result<()> {
        self.set_file_location(location)?;
        self.file.read_exact(buf)
    }

    pub fn read_parse_xml(&mut self, location: u64, size: u64) -> io::Result<HashMap<String, String>> {
        let mut buf = vec![0; size as usize];
        self.read_from_file(location, &mut buf)?;
        let buf_16_str = buf.chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<u16>>();
        
        Ok(parse_single_xml( String::from_utf16_lossy(&buf_16_str).as_str() ))
    }
}