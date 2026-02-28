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
use zstd::bulk::compress as zstd_compress;

use crate::error::Result;
use crate::mdx_conversion::records::RecordSection as MdxRecordSection;
use crate::random_access_key_blocks::upper_bound_from_prefix;
use crate::Mdict;

const READINGS_ZSTD_LEVEL: i32 = 10;
const READINGS_TARGET_UNCOMPRESSED_BLOCK_SIZE: usize = 64 * 1024;

fn write_readings_data_and_collect_key_offsets(
    readings_list: &HashMap<u64, HashSet<String>>,
    link_order: &[u64],
    link_remap: &HashMap<u64, u64>,
    readings_path: impl AsRef<Path>,
) -> Result<HashMap<String, u64>> {
    let mut key_link_map = HashMap::new();

    let mut uncompressed_offset = 0u64;
    let mut pending_block = Vec::<u8>::new();
    let mut compressed_blocks = Vec::<Vec<u8>>::new();
    let mut block_prefix_sum = vec![(0u64, 0u64)];

    let mut total_compressed = 0u64;
    let mut total_uncompressed = 0u64;

    let flush_block = |pending_block: &mut Vec<u8>,
                       compressed_blocks: &mut Vec<Vec<u8>>,
                       block_prefix_sum: &mut Vec<(u64, u64)>,
                       total_compressed: &mut u64,
                       total_uncompressed: &mut u64|
     -> Result<()> {
        if pending_block.is_empty() {
            return Ok(());
        }

        let compressed_block = zstd_compress(pending_block, READINGS_ZSTD_LEVEL)?;
        *total_compressed += compressed_block.len() as u64;
        *total_uncompressed += pending_block.len() as u64;
        block_prefix_sum.push((*total_compressed, *total_uncompressed));
        compressed_blocks.push(compressed_block);
        pending_block.clear();
        Ok(())
    };

    for &old_link in link_order {
        let Some(indices) = readings_list.get(&old_link) else {
            continue;
        };
        let remapped_link = *link_remap.get(&old_link).ok_or_else(|| {
            crate::error::MDictError::InvalidArgument(format!(
                "missing remapped link for old link {}",
                old_link
            ))
        })?;
        let indices_combined = indices.iter().cloned().collect::<Vec<String>>().join("\0");
        let indices_bytes = indices_combined.as_bytes();

        let mut entry_bytes = Vec::with_capacity(12 + indices_bytes.len());
        entry_bytes.extend_from_slice(&(indices_bytes.len() as u32).to_le_bytes());
        entry_bytes.extend_from_slice(&remapped_link.to_le_bytes());
        entry_bytes.extend_from_slice(indices_bytes);

        if !pending_block.is_empty()
            && pending_block.len() + entry_bytes.len() > READINGS_TARGET_UNCOMPRESSED_BLOCK_SIZE
        {
            flush_block(
                &mut pending_block,
                &mut compressed_blocks,
                &mut block_prefix_sum,
                &mut total_compressed,
                &mut total_uncompressed,
            )?;
        }

        pending_block.extend_from_slice(&entry_bytes);

        for index in indices {
            key_link_map
                .entry(index.clone())
                .or_insert(uncompressed_offset);
        }

        uncompressed_offset += entry_bytes.len() as u64;
    }

    flush_block(
        &mut pending_block,
        &mut compressed_blocks,
        &mut block_prefix_sum,
        &mut total_compressed,
        &mut total_uncompressed,
    )?;

    let mut readings_output_file = File::create(readings_path)?;
    readings_output_file.write_all(&(block_prefix_sum.len() as u64).to_le_bytes())?;
    for (compressed_end, uncompressed_end) in &block_prefix_sum {
        readings_output_file.write_all(&compressed_end.to_le_bytes())?;
        readings_output_file.write_all(&uncompressed_end.to_le_bytes())?;
    }

    for compressed_block in compressed_blocks {
        readings_output_file.write_all(&compressed_block)?;
    }

    Ok(key_link_map)
}

fn write_fst_map(
    key_link_map: &HashMap<String, u64>,
    output_path: impl AsRef<Path>,
) -> Result<()> {
    let output_file = File::create(output_path)?;
    let mut builder = MapBuilder::new(BufWriter::new(output_file))?;

    let mut sorted_keys: Vec<_> = key_link_map.keys().cloned().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        if let Some(&value) = key_link_map.get(&key) {
            builder.insert(key, value)?;
        }
    }

    builder.finish()?;
    Ok(())
}

fn write_record_section<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    readings_list: &HashMap<u64, HashSet<String>>,
    link_order: &[u64],
    record_output_path: impl AsRef<Path>,
) -> Result<HashMap<u64, u64>> {

    let record_output_file = File::create(record_output_path)?;
    let mut record_writer = BufWriter::new(record_output_file);

    let link_remap = MdxRecordSection::rebuild_compacted_zstd_from_mdict(
        mdict,
        readings_list,
        link_order,
        &mut record_writer,
    )?;
    record_writer.flush()?;

    Ok(link_remap)
}

fn build_sorted_key_link_order(
    readings_list: &HashMap<u64, HashSet<String>>,
) -> Result<Vec<u64>> {
    let mut key_to_links = HashMap::<String, Vec<u64>>::new();

    for (&old_link, keys) in readings_list {
        for key in keys {
            let entry = key_to_links.entry(key.clone()).or_default();
            if !entry.contains(&old_link) {
                entry.push(old_link);
            }
        }
    }

    let mut sorted_keys: Vec<String> = key_to_links.keys().cloned().collect();
    sorted_keys.sort();

    let mut seen_links = HashSet::new();
    let mut link_order = Vec::new();
    for key in sorted_keys {
        let mut links = key_to_links.remove(&key).unwrap_or_default();
        links.sort_unstable();
        for link in links {
            if seen_links.insert(link) {
                link_order.push(link);
            }
        }
    }

    Ok(link_order)
}

pub fn create_fst_index<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    readings_list: &HashMap<u64, HashSet<String>>,
    output_path: impl AsRef<Path>,
    readings_path: impl AsRef<Path>,
    record_output_path: impl AsRef<Path>,
) -> Result<()> {
    let link_order = build_sorted_key_link_order(readings_list)?;
    let link_remap = write_record_section(mdict, readings_list, &link_order, record_output_path)?;
    let key_link_map = write_readings_data_and_collect_key_offsets(
        readings_list,
        &link_order,
        &link_remap,
        readings_path,
    )?;
    write_fst_map(&key_link_map, output_path)?;

    Ok(())
}
