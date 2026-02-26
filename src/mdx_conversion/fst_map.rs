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
    values: Mmap,
    readings_file: Option<Mmap>,
}

impl FSTMap {
    pub fn load_from_path(
        path: impl AsRef<Path>,
        prefix_path: impl AsRef<Path>,
        record_path: impl AsRef<Path>,
    ) -> crate::error::Result<Self> {
        let mmap = unsafe { memmap2::Mmap::map(&File::open(path)?) }?;
        let map = Map::new(mmap)?;

        let values = unsafe { memmap2::Mmap::map(&File::open(prefix_path)?) }?;

        Ok(Self { map, values })
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

    fn value_slice(&self) -> crate::error::Result<&[u64]> {
        Ok(try_cast_slice::<u8, u64>(&self.values)?)
    }

    pub fn get_record_size(&self, link: u64) -> Option<usize> {
        let values = self.value_slice().ok()?;

        let idx = values.partition_point(|&v| v < link);
        if idx < values.len() && values[idx] == link {
            println!("Found link {} at index {}", link, idx);
            Some(values[idx + 1] as usize - values[idx] as usize)
        } else {
            None
        }
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
