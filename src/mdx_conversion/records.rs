use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom, Write},
    mem::size_of,
};

use binrw::{BinRead, BinWrite};
use minilzo_rs::adler32;
use zstd::bulk::compress as zstd_compress;

use crate::error::{MDictError, Result};
use crate::Mdict;

#[derive(BinRead, BinWrite, Debug, Clone, PartialEq, Eq)]
#[br(big)]
#[bw(big)]
pub struct RecordIndex {
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

#[derive(BinRead, BinWrite, Debug, Clone)]
#[br(big)]
#[bw(big)]
pub struct RecordSection {
    pub record_data_offset: u64,
    pub num_record_blocks: u64,
    pub num_entries: u64,
    pub byte_size_record_index: u64,
    pub byte_size_record_data: u64,
    pub num_record_indices: u64,
    #[br(count = num_record_indices)]
    pub record_index_prefix_sum: Vec<RecordIndex>,
}

impl RecordSection {
    /// Parse a record section from the reader, but without versioning since this is for MDX
    pub fn parse<R: Read + Seek>(
        reader: &mut R,
        offset: u64,
    ) -> Result<RecordSection> {
        // Seek to the offset and read the entire section using binrw's built-in functionality
        reader.seek(std::io::SeekFrom::Start(offset))?;

        // Read the complete RecordSection structure directly using binrw
        let record_section: RecordSection = RecordSection::read_le(reader)?;

        Ok(record_section)
    }

    fn rebased_record_data_offset(&self, section_offset: u64) -> u64 {
        section_offset + 6 * size_of::<u64>() as u64 + self.num_record_indices * size_of::<RecordIndex>() as u64
    }

    /// Convert from the old format to the new format
    pub fn from_old_format(old_section: &crate::format::records::RecordSection) -> RecordSection {
        // Create a new RecordIndex vector with proper prefix sum calculation
        let mut record_indices = Vec::with_capacity(old_section.record_index_prefix_sum.len());

        for i in 0..old_section.record_index_prefix_sum.len() {
            record_indices.push(RecordIndex {
                compressed_size: old_section.record_index_prefix_sum[i].compressed_size,
                uncompressed_size: old_section.record_index_prefix_sum[i].uncompressed_size,
            });
        }

        // Create the new RecordSection with proper values
        RecordSection {
            record_data_offset: old_section.record_data_offset,
            num_record_blocks: old_section.num_record_blocks,
            num_entries: old_section.num_entries,
            byte_size_record_index: old_section.byte_size_record_index,
            byte_size_record_data: old_section.byte_size_record_data,
            num_record_indices: record_indices.len() as u64,
            record_index_prefix_sum: record_indices,
        }
    }

    /// Binary-search for the record index containing `offset` (uncompressed offset)
    pub fn bin_search_record_index(&self, offset: u64) -> u64 {
        let idx = self
            .record_index_prefix_sum
            .binary_search_by_key(&offset, |ri| ri.uncompressed_size)
            .unwrap_or_else(|x| x - 1) as u64;
        idx
    }

    /// Decode a single record payload using an uncompressed `link` offset and expected `record_size`.
    ///
    /// `section_offset` is the byte offset where this RecordSection starts in `reader`.
    /// For standalone `record_section.dat`, pass `0`.
    pub fn decode_record<R: Read + Seek>(
        &self,
        reader: &mut R,
        section_offset: u64,
        link: u64,
        record_size: Option<u64>,
    ) -> Result<Vec<u8>> {
        if self.record_index_prefix_sum.len() < 2 {
            return Err(MDictError::InvalidFormat(
                "record index prefix sum is empty".to_string(),
            ));
        }

        let rec_block = self.bin_search_record_index(link) as usize;
        if rec_block + 1 >= self.record_index_prefix_sum.len() {
            return Err(MDictError::InvalidArgument(format!(
                "record block index out of range: {}",
                rec_block
            )));
        }

        let start_comp = self.record_index_prefix_sum[rec_block].compressed_size;
        let end_comp = self.record_index_prefix_sum[rec_block + 1].compressed_size;
        let comp_size = (end_comp - start_comp) as usize;

        let record_data_offset = self.rebased_record_data_offset(section_offset);
        let read_offset = record_data_offset + start_comp;

        let mut comp_buf = vec![0u8; comp_size];
        reader.seek(SeekFrom::Start(read_offset))?;
        reader.read_exact(&mut comp_buf)?;

        let decomp = crate::format::decode_format_block(&comp_buf)?;

        let uncompressed_before = self.record_index_prefix_sum[rec_block].uncompressed_size;
        if link < uncompressed_before {
            return Err(MDictError::InvalidArgument(format!(
                "link {} is before block start {}",
                link, uncompressed_before
            )));
        }

        let start = (link - uncompressed_before) as usize;
        if start > decomp.len() {
            return Err(MDictError::InvalidFormat(format!(
                "decoded offset {} out of bounds for block size {}",
                start,
                decomp.len()
            )));
        }

        let end = start.saturating_add(record_size.unwrap_or(decomp.len() as u64) as usize).min(decomp.len());
        Ok(decomp[start..end].to_vec())
    }

    pub fn write_to<W: Write + Seek, R: Read + Seek>(&self, writer: &mut W, old_file: &mut R) -> Result<()> {
        self.write_le(writer)?;
        
        // Write all contents of old_file starting from record_data_offset to the end of the file
        old_file.seek(std::io::SeekFrom::Start(self.record_data_offset))?;

        std::io::copy(old_file, writer)?;
        
        Ok(())
    }

    pub fn detect_record_indexes_never_used(&self, readings_list: &HashMap<u64, HashSet<String>>) -> u64 {
        let mut used_blocks = HashSet::new();

        for &link in readings_list.keys() {
            let rec_block = self.bin_search_record_index(link) as usize;
            used_blocks.insert(rec_block);
        }

        let mut never_used = Vec::new();
        for i in 0..(self.record_index_prefix_sum.len() - 1) {
            if !used_blocks.contains(&i) {
                never_used.push(i);
            }
        }

        let compressed_size_saved: u64 = never_used.iter().map(|&i| {
            let start_comp = self.record_index_prefix_sum[i].compressed_size;
            let end_comp = self.record_index_prefix_sum[i + 1].compressed_size;
            end_comp - start_comp
        }).sum();
        
        compressed_size_saved
    }

    pub fn rebuild_compacted_zstd_from_mdict<R: Read + Seek, W: Write + Seek>(
        mdict: &mut Mdict<R>,
        readings_list: &HashMap<u64, HashSet<String>>,
        ordered_old_links: &[u64],
        writer: &mut W,
    ) -> Result<HashMap<u64, u64>> {
        let record_sizes = build_record_sizes_from_key_index(mdict)?;
        let mut decode_cache: HashMap<usize, Vec<u8>> = HashMap::new();

        let mut seen = HashSet::new();
        let mut records: Vec<(u64, Vec<u8>)> = Vec::new();

        for &old_link in ordered_old_links {
            if !readings_list.contains_key(&old_link) || !seen.insert(old_link) {
                continue;
            }

            let record_size = *record_sizes.get(&old_link).ok_or_else(|| {
                MDictError::InvalidArgument(format!("missing record size for link {}", old_link))
            })?;
            let record = decode_record_by_link(mdict, &mut decode_cache, old_link, record_size)?;
            records.push((old_link, record));
        }

        if records.is_empty() {
            return Err(MDictError::InvalidArgument(
                "no referenced records found for compaction".to_string(),
            ));
        }

        let (new_section, compressed_data, link_remap) =
            build_compacted_zstd_section(&mdict.record_section, records)?;

        new_section.write_le(writer)?;
        writer.write_all(&compressed_data)?;

        Ok(link_remap)
    }
}

const ZSTD_ENCODING: u32 = 4;
const ZSTD_LEVEL: i32 = 10;
const TARGET_UNCOMPRESSED_BLOCK_SIZE: usize = 64 * 1024;

fn build_record_sizes_from_key_index<R: Read + Seek>(
    mdict: &mut Mdict<R>,
) -> Result<HashMap<u64, u64>> {
    let total_entries = mdict.key_block_index.key_section.num_entries as usize;
    let mut key_ids = Vec::with_capacity(total_entries);

    for i in 0..total_entries {
        let Some(key_block) = mdict.key_block_index.get(&mut mdict.reader, i)? else {
            break;
        };
        key_ids.push(key_block.key_id);
    }

    let total_uncompressed_size = mdict
        .record_section
        .record_index_prefix_sum
        .last()
        .map(|idx| idx.uncompressed_size)
        .ok_or_else(|| MDictError::InvalidFormat("missing record index prefix sum".to_string()))?;

    let mut sizes = HashMap::with_capacity(key_ids.len());
    for i in 0..key_ids.len() {
        let current = key_ids[i];
        let next = key_ids
            .get(i + 1)
            .copied()
            .unwrap_or(total_uncompressed_size);
        if next < current {
            return Err(MDictError::InvalidFormat(format!(
                "non-monotonic key ids at {}: {} -> {}",
                i, current, next
            )));
        }
        sizes.insert(current, next - current);
    }

    Ok(sizes)
}

fn decode_record_block_cached<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    decode_cache: &mut HashMap<usize, Vec<u8>>,
    rec_block: usize,
) -> Result<Vec<u8>> {
    if !decode_cache.contains_key(&rec_block) {
        if rec_block + 1 >= mdict.record_section.record_index_prefix_sum.len() {
            return Err(MDictError::InvalidArgument(format!(
                "record block index out of range: {}",
                rec_block
            )));
        }

        let start_comp = mdict.record_section.record_index_prefix_sum[rec_block].compressed_size;
        let end_comp = mdict.record_section.record_index_prefix_sum[rec_block + 1].compressed_size;
        let comp_size = (end_comp - start_comp) as usize;

        let read_offset = mdict.record_section.record_data_offset + start_comp;
        let mut comp_buf = vec![0u8; comp_size];
        mdict.reader.seek(SeekFrom::Start(read_offset))?;
        mdict.reader.read_exact(&mut comp_buf)?;

        let decomp = crate::format::decode_format_block(&comp_buf)?;
        decode_cache.insert(rec_block, decomp);
    }

    decode_cache
        .get(&rec_block)
        .cloned()
        .ok_or_else(|| MDictError::InvalidFormat("missing decoded record block".to_string()))
}

fn decode_record_by_link<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    decode_cache: &mut HashMap<usize, Vec<u8>>,
    link: u64,
    record_size: u64,
) -> Result<Vec<u8>> {
    let mut remaining = record_size as usize;
    let mut current_abs = link;
    let mut out = Vec::with_capacity(remaining);

    while remaining > 0 {
        let rec_block = mdict.record_section.bin_search_record_index(current_abs) as usize;
        let block_start_uncompressed =
            mdict.record_section.record_index_prefix_sum[rec_block].uncompressed_size;
        let block = decode_record_block_cached(mdict, decode_cache, rec_block)?;

        let start_in_block = (current_abs - block_start_uncompressed) as usize;
        if start_in_block > block.len() {
            return Err(MDictError::InvalidFormat(format!(
                "decoded offset {} out of bounds for block size {}",
                start_in_block,
                block.len()
            )));
        }

        let take = remaining.min(block.len().saturating_sub(start_in_block));
        if take == 0 {
            return Err(MDictError::InvalidFormat(
                "unable to advance while decoding record".to_string(),
            ));
        }

        out.extend_from_slice(&block[start_in_block..start_in_block + take]);
        remaining -= take;
        current_abs += take as u64;
    }

    if out.ends_with(&[0x0A, 0x00]) {
        out.truncate(out.len().saturating_sub(2));
    }

    Ok(out)
}

fn encode_zstd_block(uncompressed: &[u8]) -> Result<Vec<u8>> {
    let compressed = zstd_compress(uncompressed, ZSTD_LEVEL)?;
    let checksum = adler32(uncompressed);

    let mut out = Vec::with_capacity(8 + 4 + compressed.len());
    out.extend_from_slice(&ZSTD_ENCODING.to_le_bytes());
    out.extend_from_slice(&checksum.to_be_bytes());
    out.extend_from_slice(&(uncompressed.len() as u32).to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

fn flush_pending_records_as_block(
    pending_records: &mut Vec<(u64, Vec<u8>)>,
    pending_uncompressed_size: &mut usize,
    compressed_data: &mut Vec<u8>,
    prefix_sum: &mut Vec<RecordIndex>,
    link_remap: &mut HashMap<u64, u64>,
    total_uncompressed: &mut u64,
    total_compressed: &mut u64,
    block_count: &mut u64,
) -> Result<()> {
    if pending_records.is_empty() {
        return Ok(());
    }

    let mut block_uncompressed = Vec::with_capacity(*pending_uncompressed_size);
    for (old_link, record) in pending_records.drain(..) {
        let new_link = *total_uncompressed + block_uncompressed.len() as u64;
        link_remap.insert(old_link, new_link);
        block_uncompressed.extend_from_slice(&record);
    }

    let encoded = encode_zstd_block(&block_uncompressed)?;
    *total_compressed += encoded.len() as u64;
    *total_uncompressed += block_uncompressed.len() as u64;

    compressed_data.extend_from_slice(&encoded);
    prefix_sum.push(RecordIndex {
        compressed_size: *total_compressed,
        uncompressed_size: *total_uncompressed,
    });

    *pending_uncompressed_size = 0;
    *block_count += 1;
    Ok(())
}

fn build_compacted_zstd_section(
    old_section: &crate::format::records::RecordSection,
    records: Vec<(u64, Vec<u8>)>,
) -> Result<(RecordSection, Vec<u8>, HashMap<u64, u64>)> {
    let mut compressed_data = Vec::new();
    let mut prefix_sum = vec![RecordIndex {
        compressed_size: 0,
        uncompressed_size: 0,
    }];

    let mut link_remap = HashMap::with_capacity(records.len());
    let mut pending_records: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut pending_uncompressed_size = 0usize;
    let mut total_uncompressed = 0u64;
    let mut total_compressed = 0u64;
    let mut block_count = 0u64;

    for (old_link, record) in records {
        if !pending_records.is_empty()
            && pending_uncompressed_size + record.len() > TARGET_UNCOMPRESSED_BLOCK_SIZE
        {
            flush_pending_records_as_block(
                &mut pending_records,
                &mut pending_uncompressed_size,
                &mut compressed_data,
                &mut prefix_sum,
                &mut link_remap,
                &mut total_uncompressed,
                &mut total_compressed,
                &mut block_count,
            )?;
        }

        pending_uncompressed_size += record.len();
        pending_records.push((old_link, record));
    }

    flush_pending_records_as_block(
        &mut pending_records,
        &mut pending_uncompressed_size,
        &mut compressed_data,
        &mut prefix_sum,
        &mut link_remap,
        &mut total_uncompressed,
        &mut total_compressed,
        &mut block_count,
    )?;

    if block_count == 0 {
        return Err(MDictError::InvalidFormat(
            "no records were written into compacted section".to_string(),
        ));
    }

    let num_record_indices = prefix_sum.len() as u64;
    let byte_size_record_index = num_record_indices * size_of::<RecordIndex>() as u64;

    let section = RecordSection {
        record_data_offset: 0,
        num_record_blocks: block_count,
        num_entries: old_section.num_entries,
        byte_size_record_index,
        byte_size_record_data: total_compressed,
        num_record_indices,
        record_index_prefix_sum: prefix_sum,
    };

    Ok((section, compressed_data, link_remap))
}
