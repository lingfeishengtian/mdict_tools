use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::Path;

use bytemuck::try_cast_slice;
use fst::map::{Stream, StreamBuilder};
use fst::{IntoStreamer, Map, MapBuilder, Streamer};
use memmap2::Mmap;
use sorted_vec::{SortedSet, SortedVec};

use crate::mdx_conversion::records::RecordSection as MdxRecordSection;
use crate::random_access_key_blocks::upper_bound_from_prefix;
use crate::Mdict;

pub struct FSTMap {
    map: Map<Mmap>,
    readings_file: Mmap,
    record_section: MdxRecordSection,
}

#[derive(Debug)]
pub struct ReadingsEntry {
    pub length: u32,
    pub link_id: u64,
    pub readings: Vec<String>,
}

impl FSTMap {
    pub fn load_from_path(
        path: impl AsRef<Path>,
        readings_path: impl AsRef<Path>,
        record_path: impl AsRef<Path>,
    ) -> crate::error::Result<Self> {
        let mmap = unsafe { memmap2::Mmap::map(&File::open(path)?) }?;
        let map = Map::new(mmap)?;

        let readings_file = unsafe { memmap2::Mmap::map(&File::open(readings_path)?) }?;

        // Load the record section
        let mut record_file = File::open(record_path)?;
        let record_section = MdxRecordSection::parse(&mut record_file, 0)?;

        Ok(Self {
            map,
            readings_file,
            record_section,
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

    pub fn get_record<R: Read + Seek>(
        &self,
        readings_offset: u64,
        reader: &mut R,
        record_size: Option<u64>,
    ) -> Option<Vec<u8>> {
        let (readings_entry, size_from_readings) = self.get_readings(readings_offset)?;
        let effective_size = record_size.or(size_from_readings);

        self.record_section
            .decode_record(reader, 0, readings_entry.link_id, effective_size)
            .ok()
    }

    pub fn get_readings(&self, offset: u64) -> Option<(ReadingsEntry, Option<u64>)> {
        let readings_file = &self.readings_file;
        let offset = usize::try_from(offset).ok()?;
        let header_end = offset.checked_add(12)?;

        if header_end > readings_file.len() {
            return None;
        }

        let length = u32::from_le_bytes(readings_file[offset..offset + 4].try_into().ok()?) as usize;
        let link_id = u64::from_le_bytes(readings_file[offset + 4..offset + 12].try_into().ok()?);

        let string_start = header_end;
        let string_end = string_start.checked_add(length)?;

        if string_end > readings_file.len() {
            return None;
        }

        let readings = readings_file[string_start..string_end]
            .split(|&byte| byte == 0)
            .filter(|segment| !segment.is_empty())
            .filter_map(|segment| std::str::from_utf8(segment).ok())
            .map(str::to_owned)
            .collect();

        let next_link_offset = string_end.checked_add(4)?;
        let record_size = next_link_offset
            .checked_add(8)
            .filter(|&next_link_end| next_link_end <= readings_file.len())
            .and_then(|next_link_end| {
                let next_link_id =
                    u64::from_le_bytes(readings_file[next_link_offset..next_link_end].try_into().ok()?);
                Some(next_link_id - link_id)
            });

        Some((
            ReadingsEntry {
                length: length as u32,
                link_id,
                readings,
            },
            record_size,
        ))
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
