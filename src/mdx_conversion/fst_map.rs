use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

use bytemuck::try_cast_slice;
use fst::map::{Stream, StreamBuilder};
use fst::{IntoStreamer, Map, MapBuilder, Streamer};
use memmap2::Mmap;
use sorted_vec::{SortedSet, SortedVec};
use zstd::bulk::decompress as zstd_decompress;

use crate::mdx_conversion::records::RecordSection as MdxRecordSection;
use crate::random_access_key_blocks::upper_bound_from_prefix;
use crate::Mdict;

#[derive(Clone, Copy)]
struct ReadingsBlockIndex {
    compressed_end: u64,
    uncompressed_end: u64,
}

enum ReadingsStorage {
    BlockCompressed {
        data_offset: usize,
        block_index_prefix_sum: Vec<ReadingsBlockIndex>,
    },
}

const READINGS_BLOCK_CACHE_CAPACITY: usize = 8;

#[derive(Clone)]
struct CachedReadingsBlock {
    block_pos: usize,
    uncompressed_start: usize,
    uncompressed_end: usize,
    block: Arc<[u8]>,
}

#[derive(Default)]
struct ReadingsBlockCache {
    entries: VecDeque<CachedReadingsBlock>,
}

impl ReadingsBlockCache {
    fn get(&mut self, block_pos: usize) -> Option<CachedReadingsBlock> {
        let idx = self.entries.iter().position(|e| e.block_pos == block_pos)?;
        let entry = self.entries.remove(idx)?;
        self.entries.push_front(entry.clone());
        Some(entry)
    }

    fn put(&mut self, entry: CachedReadingsBlock) {
        if let Some(existing_idx) = self.entries.iter().position(|e| e.block_pos == entry.block_pos) {
            let _ = self.entries.remove(existing_idx);
        }
        self.entries.push_front(entry);
        while self.entries.len() > READINGS_BLOCK_CACHE_CAPACITY {
            let _ = self.entries.pop_back();
        }
    }
}

pub struct FSTMap {
    map: Map<Mmap>,
    readings_file: Mmap,
    readings_storage: ReadingsStorage,
    readings_block_cache: RefCell<ReadingsBlockCache>,
    record_section: MdxRecordSection,
}

#[derive(Debug)]
pub struct ReadingsEntry {
    pub length: u32,
    pub link_id: u64,
    pub readings: Vec<String>,
}

impl FSTMap {
    fn parse_readings_storage(readings_file: &Mmap) -> crate::error::Result<ReadingsStorage> {
        if readings_file.len() < 8 {
            return Err(crate::error::MDictError::InvalidFormat(
                "readings file too small for header".to_string(),
            ));
        }

        let num_indices =
            u64::from_le_bytes(readings_file[0..8].try_into().map_err(|_| {
                crate::error::MDictError::InvalidFormat("invalid readings index count".to_string())
            })?) as usize;

        let index_bytes = num_indices.checked_mul(16).ok_or_else(|| {
            crate::error::MDictError::InvalidFormat("readings index overflow".to_string())
        })?;
        let data_offset = 8usize.checked_add(index_bytes).ok_or_else(|| {
            crate::error::MDictError::InvalidFormat("readings header overflow".to_string())
        })?;

        if data_offset > readings_file.len() {
            return Err(crate::error::MDictError::InvalidFormat(
                "readings header exceeds file size".to_string(),
            ));
        }

        let mut block_index_prefix_sum = Vec::with_capacity(num_indices);
        let mut cursor = 8usize;
        for _ in 0..num_indices {
            let compressed_end = u64::from_le_bytes(
                readings_file[cursor..cursor + 8]
                    .try_into()
                    .map_err(|_| {
                        crate::error::MDictError::InvalidFormat(
                            "invalid compressed index bytes".to_string(),
                        )
                    })?,
            );
            let uncompressed_end = u64::from_le_bytes(
                readings_file[cursor + 8..cursor + 16]
                    .try_into()
                    .map_err(|_| {
                        crate::error::MDictError::InvalidFormat(
                            "invalid uncompressed index bytes".to_string(),
                        )
                    })?,
            );
            block_index_prefix_sum.push(ReadingsBlockIndex {
                compressed_end,
                uncompressed_end,
            });
            cursor += 16;
        }

        if block_index_prefix_sum.is_empty() {
            return Err(crate::error::MDictError::InvalidFormat(
                "empty readings block index".to_string(),
            ));
        }

        Ok(ReadingsStorage::BlockCompressed {
            data_offset,
            block_index_prefix_sum,
        })
    }

    pub fn load_from_path(
        path: impl AsRef<Path>,
        readings_path: impl AsRef<Path>,
        record_path: impl AsRef<Path>,
    ) -> crate::error::Result<Self> {
        let mmap = unsafe { memmap2::Mmap::map(&File::open(path)?) }?;
        let map = Map::new(mmap)?;

        let readings_file = unsafe { memmap2::Mmap::map(&File::open(readings_path)?) }?;
        let readings_storage = Self::parse_readings_storage(&readings_file)?;

        // Load the record section
        let mut record_file = File::open(record_path)?;
        let record_section = MdxRecordSection::parse(&mut record_file, 0)?;

        Ok(Self {
            map,
            readings_file,
            readings_storage,
            readings_block_cache: RefCell::new(ReadingsBlockCache::default()),
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

    fn read_uncompressed_block_at_offset(
        &self,
        offset: u64,
    ) -> Option<(Arc<[u8]>, usize, usize)> {
        let ReadingsStorage::BlockCompressed {
            data_offset,
            block_index_prefix_sum,
        } = &self.readings_storage;

        if block_index_prefix_sum.len() < 2 {
            return None;
        }

        let block_pos = block_index_prefix_sum
            .partition_point(|idx| idx.uncompressed_end <= offset);
        if block_pos == 0 || block_pos >= block_index_prefix_sum.len() {
            return None;
        }

        if let Some(cached) = self.readings_block_cache.borrow_mut().get(block_pos) {
            return Some((
                cached.block,
                cached.uncompressed_start,
                cached.uncompressed_end,
            ));
        }

        let prev = block_index_prefix_sum[block_pos - 1];
        let cur = block_index_prefix_sum[block_pos];

        let compressed_start = usize::try_from(prev.compressed_end).ok()?;
        let compressed_end = usize::try_from(cur.compressed_end).ok()?;
        let uncompressed_start = usize::try_from(prev.uncompressed_end).ok()?;
        let uncompressed_end = usize::try_from(cur.uncompressed_end).ok()?;

        if compressed_end < compressed_start || uncompressed_end < uncompressed_start {
            return None;
        }

        let data_start = data_offset.checked_add(compressed_start)?;
        let data_end = data_offset.checked_add(compressed_end)?;
        if data_end > self.readings_file.len() || data_start > data_end {
            return None;
        }

        let expected_size = uncompressed_end - uncompressed_start;
        let block: Arc<[u8]> = zstd_decompress(&self.readings_file[data_start..data_end], expected_size)
            .ok()?
            .into();

        self.readings_block_cache.borrow_mut().put(CachedReadingsBlock {
            block_pos,
            uncompressed_start,
            uncompressed_end,
            block: block.clone(),
        });

        Some((block, uncompressed_start, uncompressed_end))
    }

    fn parse_readings_from_uncompressed_offset(
        &self,
        offset: u64,
    ) -> Option<(ReadingsEntry, u64)> {
        let (block, block_uncompressed_start, _) = self.read_uncompressed_block_at_offset(offset)?;
        let offset_usize = usize::try_from(offset).ok()?;
        let local_offset = offset_usize.checked_sub(block_uncompressed_start)?;
        let header_end = local_offset.checked_add(12)?;
        if header_end > block.len() {
            return None;
        }

        let length = u32::from_le_bytes(block[local_offset..local_offset + 4].try_into().ok()?) as usize;
        let link_id = u64::from_le_bytes(block[local_offset + 4..local_offset + 12].try_into().ok()?);
        let string_start = header_end;
        let string_end = string_start.checked_add(length)?;
        if string_end > block.len() {
            return None;
        }

        let readings = block[string_start..string_end]
            .split(|&byte| byte == 0)
            .filter(|segment| !segment.is_empty())
            .filter_map(|segment| std::str::from_utf8(segment).ok())
            .map(str::to_owned)
            .collect();

        let entry_size = 12u64 + length as u64;
        Some((
            ReadingsEntry {
                length: length as u32,
                link_id,
                readings,
            },
            entry_size,
        ))
    }

    fn get_next_link_id_from_uncompressed_offset(&self, offset: u64) -> Option<u64> {
        let ReadingsStorage::BlockCompressed {
            block_index_prefix_sum,
            ..
        } = &self.readings_storage;

        let total_uncompressed = block_index_prefix_sum.last()?.uncompressed_end;
        if offset >= total_uncompressed {
            return None;
        }

        let (entry, _) = self.parse_readings_from_uncompressed_offset(offset)?;
        Some(entry.link_id)
    }

    pub fn get_readings(&self, offset: u64) -> Option<(ReadingsEntry, Option<u64>)> {
        let (entry, entry_size) = self.parse_readings_from_uncompressed_offset(offset)?;
        let next_offset = offset.checked_add(entry_size)?;
        let next_link = self.get_next_link_id_from_uncompressed_offset(next_offset);
        let record_size = next_link.map(|next_link_id| next_link_id - entry.link_id);
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
