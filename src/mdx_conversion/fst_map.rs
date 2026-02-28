use std::cell::RefCell;
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;

use fst::map::Stream;
use fst::{IntoStreamer, Map, Streamer};
use memmap2::Mmap;

use crate::error::Result;
use crate::mdx_conversion::readings::{
    read_entry_from_bytes, read_header_from_bytes, ReadingsEntry,
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
    fn parse_readings_from_uncompressed_offset(&self, offset: u64) -> Option<(ReadingsEntry, u64)> {
        if offset >= self.readings_mmap.len() as u64 {
            return None;
        }
        let entry = read_entry_from_bytes(&self.readings_mmap, offset)?;
        let entry_size = entry.entry_size;
        Some((entry, entry_size))
    }

    fn get_next_link_id_from_uncompressed_offset(&self, offset: u64) -> Option<u64> {
        if offset >= self.readings_mmap.len() as u64 {
            return None;
        }
        read_header_from_bytes(&self.readings_mmap, offset).map(|header| header.link_id)
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

    pub fn load_from_path_with_cache(
        path: impl AsRef<Path>,
        readings_path: impl AsRef<Path>,
        record_path: impl AsRef<Path>,
        _readings_cache_blocks: usize,
    ) -> Result<Self> {
        Self::load_from_path(path, readings_path, record_path)
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

    pub fn get_record(
        &self,
        readings_offset: u64,
        record_size: Option<u64>,
    ) -> Option<Vec<u8>> {
        let (readings_entry, size_from_readings) = self.get_readings(readings_offset)?;
        let effective_size = record_size.or(size_from_readings);
        let mut record_file = self.record_file.borrow_mut();
        self.record_section
            .decode_record(&mut *record_file, readings_entry.link_id, effective_size)
            .ok()
    }

    pub fn get_readings(&self, offset: u64) -> Option<(ReadingsEntry, Option<u64>)> {
        let (entry, entry_size) = self.parse_readings_from_uncompressed_offset(offset)?;
        let next_offset = offset.checked_add(entry_size)?;
        let next_link = self.get_next_link_id_from_uncompressed_offset(next_offset);
        let record_size = next_link.and_then(|next_link_id| next_link_id.checked_sub(entry.link_id));

        Some((entry, record_size))
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
