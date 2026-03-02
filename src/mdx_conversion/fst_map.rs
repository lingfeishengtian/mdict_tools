use std::cell::RefCell;
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;

use fst::map::Stream;
use fst::{IntoStreamer, Map, Streamer};
use memmap2::Mmap;

use crate::error::{MDictError, Result};
use crate::mdx_conversion::readings::{
    read_entry_from_bytes_result, read_header_from_bytes_result, ReadingsEntry,
};
use crate::mdx_conversion::records::RecordSection as MdxRecordSection;
use crate::random_access_key_blocks::upper_bound_from_prefix;

pub struct FSTMap {
    map: Map<Mmap>,
    readings_mmap: Mmap,
    record_section: MdxRecordSection,
    record_file: RefCell<File>,
}

impl FSTMap {
    fn ensure_readings_offset_in_bounds(&self, offset: u64) -> Result<()> {
        if offset >= self.readings_mmap.len() as u64 {
            return Err(MDictError::InvalidArgument(format!(
                "readings offset {} out of bounds for size {}",
                offset,
                self.readings_mmap.len()
            )));
        }
        Ok(())
    }

    fn parse_readings_from_uncompressed_offset_result(
        &self,
        offset: u64,
    ) -> Result<(ReadingsEntry, u64)> {
        self.ensure_readings_offset_in_bounds(offset)?;
        let entry = read_entry_from_bytes_result(&self.readings_mmap, offset)?;
        let entry_size = entry.entry_size;
        Ok((entry, entry_size))
    }

    fn get_next_link_id_from_uncompressed_offset_result(&self, offset: u64) -> Result<u64> {
        self.ensure_readings_offset_in_bounds(offset)?;
        let header = read_header_from_bytes_result(&self.readings_mmap, offset)?;
        Ok(header.link_id)
    }

    pub fn load_from_path(
        path: impl AsRef<Path>,
        readings_path: impl AsRef<Path>,
        record_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let mmap = unsafe { memmap2::Mmap::map(&File::open(path)?) }?;
        let map = Map::new(mmap)?;

        let readings_mmap = unsafe { memmap2::Mmap::map(&File::open(readings_path)?) }?;

        let mut record_file = File::open(record_path)?;
        let record_section = MdxRecordSection::parse(&mut record_file)?;

        Ok(Self {
            map,
            readings_mmap,
            record_section,
            record_file: RefCell::new(record_file),
        })
    }

    pub fn get(&self, key: &str) -> Option<u64> {
        self.map.get(key)
    }

    pub fn get_link_for_key<'a>(&'a self, key: &'a str) -> Stream<'a> {
        self.map
            .range()
            .ge(key)
            .lt(upper_bound_from_prefix(key).unwrap())
            .into_stream()
    }

    pub fn get_link_for_key_dedup<'a>(&'a self, key: &'a str) -> DedupStream<'a> {
        DedupStream::new(self.get_link_for_key(key))
    }

    pub fn get_link_page_for_prefix(
        &self,
        prefix: &str,
        cursor_after_key: Option<&str>,
        page_size: usize,
    ) -> Result<(Vec<(String, u64)>, Option<String>)> {
        if page_size == 0 {
            return Err(MDictError::InvalidArgument(
                "page_size must be greater than 0".to_string(),
            ));
        }

        let upper_bound = upper_bound_from_prefix(prefix).ok_or_else(|| {
            MDictError::InvalidArgument(format!("invalid prefix for range bound: '{}'", prefix))
        })?;

        let mut builder = self.map.range();
        if let Some(after_key) = cursor_after_key {
            builder = builder.gt(after_key);
        } else {
            builder = builder.ge(prefix);
        }
        builder = builder.lt(&upper_bound);

        let mut stream = builder.into_stream();
        let mut results = Vec::with_capacity(page_size + 1);

        while results.len() < page_size + 1 {
            let Some((raw_key, value)) = stream.next() else {
                break;
            };
            results.push((String::from_utf8_lossy(raw_key).to_string(), value));
        }

        let has_more = results.len() > page_size;
        if has_more {
            results.truncate(page_size);
        }

        let next_cursor = if has_more {
            results.last().map(|(key, _)| key.clone())
        } else {
            None
        };

        Ok((results, next_cursor))
    }

    pub fn get_record(
        &self,
        readings_offset: u64,
        record_size: Option<u64>,
    ) -> Option<Vec<u8>> {
        self.get_record_result(readings_offset, record_size).ok()
    }

    pub fn get_record_result(
        &self,
        readings_offset: u64,
        record_size: Option<u64>,
    ) -> Result<Vec<u8>> {
        let (readings_entry, size_from_readings) = self.get_readings_result(readings_offset)?;
        let effective_size = record_size.or(size_from_readings);
        let mut record_file = self
            .record_file
            .try_borrow_mut()
            .map_err(|_| MDictError::InvalidFormat("record file is already borrowed".to_string()))?;
        self.record_section
            .decode_record(&mut *record_file, readings_entry.link_id, effective_size)
    }

    pub fn get_readings(&self, offset: u64) -> Option<(ReadingsEntry, Option<u64>)> {
        self.get_readings_result(offset).ok()
    }

    pub fn get_readings_result(&self, offset: u64) -> Result<(ReadingsEntry, Option<u64>)> {
        let (entry, entry_size) = self.parse_readings_from_uncompressed_offset_result(offset)?;
        let next_offset = offset
            .checked_add(entry_size)
            .ok_or_else(|| MDictError::InvalidFormat("readings offset overflow".to_string()))?;
        let next_link = self.get_next_link_id_from_uncompressed_offset_result(next_offset).ok();
        let record_size = next_link.and_then(|next_link_id| next_link_id.checked_sub(entry.link_id));

        Ok((entry, record_size))
    }
}

/// A wrapper around fst::Stream that skips duplicate values
pub struct DedupStream<'a> {
    stream: Stream<'a>,
    seen_values: HashSet<u64>,
}

impl<'a> DedupStream<'a> {
    pub fn new(stream: Stream<'a>) -> Self {
        Self {
            stream,
            seen_values: HashSet::new(),
        }
    }
}

impl<'a> Iterator for DedupStream<'a> {
    type Item = (String, u64);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(item) = self.stream.next() {
            if self.seen_values.insert(item.1) {
                return Some((String::from_utf8_lossy(item.0).to_string(), item.1));
            }
        }
        None
    }
}
