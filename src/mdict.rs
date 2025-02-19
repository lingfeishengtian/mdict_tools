use log::Record;
use regex::Regex;

use crate::file_reader::FileHandler;
use crate::header::parser::HeaderInfo;
use crate::key_index;
use crate::key_index::parser::{KeyBlock, KeySection};
use crate::key_index::search_result::SearchResultPointer;
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

    pub fn search_query(&mut self, query: &str) -> Option<SearchResultEnumerator> {
        let search_pointer = self.key_index.search_query(query, &mut self.file_handler);

        if let Some(search_pointer) = search_pointer {
            Some(SearchResultEnumerator::new(&mut self.file_handler, &mut self.key_index, &mut self.record, search_pointer))
        } else {
            None
        }
    }
}

pub struct SearchResultEnumerator<'a> {
    file_handler: &'a mut FileHandler,
    key_section: &'a mut KeySection,
    record_section: &'a mut RecordSection,
    search_pointer: SearchResultPointer
}

impl<'a> SearchResultEnumerator<'a> {
    pub fn new(
        file_handler: &'a mut FileHandler, 
        key_section: &'a mut KeySection, 
        record_section: &'a mut RecordSection, 
        search_pointer: SearchResultPointer
    ) -> Self {
        Self {
            file_handler,
            key_section,
            record_section,
            search_pointer
        }
    }

    pub fn next(&mut self) -> Option<(KeyBlock, String)> {
        let block = self.search_pointer.next(self.file_handler, self.key_section)?;
        let record = self.record_section.record_at_offset(block.key_id, self.file_handler);

        let re = regex::Regex::new(r"@@@LINK=([^\s]+)").unwrap();

        if let Some(captures) = re.captures(&record) {
            if let Some(link) = captures.get(1) {
                let link_text = link.as_str();
                
                // No recursive links since they take too long to unravel
                let query = self.key_section.search_query(link_text, self.file_handler);
                if let Some(mut query) = query {
                    let block = query.next(self.file_handler, self.key_section)?;
                    let record = self.record_section.record_at_offset(block.key_id, &mut self.file_handler);

                    return Some((block, record));
                }
            }
        }

        Some((block, record))
    }
}