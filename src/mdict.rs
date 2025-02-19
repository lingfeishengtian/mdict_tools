use log::Record;

use crate::file_reader::FileHandler;
use crate::header::parser::HeaderInfo;
use crate::key_index::parser::{KeyBlock, KeySection};
use crate::records::parser::RecordSection;

pub struct MDict {
    file_handler: FileHandler,
    header_info: HeaderInfo,
    key_index: KeySection,
    record: RecordSection
}

impl MDict {
    pub fn open(file_path: &str) -> Result<MDict, std::io::Error> {
        let mut file_handler = FileHandler::open(file_path)?;
        let header_info = HeaderInfo::retrieve_header(&mut file_handler)?;
        
        if !header_info.is_valid() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid header"));
        }

        let key_index = KeySection::retrieve_key_index(&mut file_handler, &header_info)?;
        let record = RecordSection::parse(&header_info, &key_index, &mut file_handler);

        Ok(MDict { file_handler, header_info, key_index, record })
    }
    
    pub fn get_header_info(&self) -> &HeaderInfo {
        &self.header_info
    }

    // pub fn search_query(&mut self, query: &str) -> Option<Vec<KeyBlock>> {
    //     self.key_index.search_query(query, &mut self.file_handler)
    // }
}