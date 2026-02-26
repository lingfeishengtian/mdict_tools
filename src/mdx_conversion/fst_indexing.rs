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

pub fn create_fst_index<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    readings_list: &HashMap<u64, HashSet<String>>,
    output_path: impl AsRef<Path>,
    readings_path: impl AsRef<Path>,
    record_output_path: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut key_link_map = HashMap::new();

    let mut readings_output_file = File::create(readings_path)?;
    let mut offset = 0u64;
    // Store in file as (# of bytes for string, link, string bytes) for each entry
    for (link, indices) in readings_list {
        let indices_combined = indices.iter().cloned().collect::<Vec<String>>().join("\0");
        let indices_bytes = indices_combined.as_bytes();

        // Write the length of the string, the link, and the string bytes to the file
        readings_output_file.write_all(&(indices_bytes.len() as u32).to_le_bytes())?;
        readings_output_file.write_all(&link.to_le_bytes())?;
        readings_output_file.write_all(indices_bytes)?;
        
        for index in indices {
            key_link_map.insert(index.clone(), offset);
        }
        offset += indices_bytes.len() as u64 + 12; // 4 bytes for length, 8 bytes for link
    }

    let output_file = File::create(output_path)?;
    let mut builder = MapBuilder::new(BufWriter::new(output_file))?;

    // key_link_map must be lexographically sorted for fst::MapBuilder
    let mut sorted_keys: Vec<_> = key_link_map.keys().cloned().collect();
    sorted_keys.sort();

    for key in sorted_keys {
        if let Some(&value) = key_link_map.get(&key) {
            builder.insert(key, value)?;
        }
    }
    builder.finish()?;

    // Convert and write the record section to a separate file
    let record_section = &mdict.record_section;
    let new_record_section = MdxRecordSection::from_old_format(record_section);

    // Write the record section manually without using binrw's write_to method that requires Seek
    let record_output_file = File::create(record_output_path)?;
    let mut record_writer = BufWriter::new(record_output_file);

    new_record_section.write_to(&mut record_writer, &mut mdict.reader)?;
    record_writer.flush()?;

    Ok(())
}
