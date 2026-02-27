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

use crate::error::Result;
use crate::mdx_conversion::records::RecordSection as MdxRecordSection;
use crate::random_access_key_blocks::upper_bound_from_prefix;
use crate::Mdict;

fn write_readings_data_and_collect_key_offsets(
    readings_list: &HashMap<u64, HashSet<String>>,
    readings_path: impl AsRef<Path>,
) -> Result<HashMap<String, u64>> {
    let mut key_link_map = HashMap::new();
    let sorted_links: SortedVec<u64> = readings_list.keys().cloned().collect();

    let mut readings_output_file = File::create(readings_path)?;
    let mut offset = 0u64;

    for link in sorted_links {
        let indices = readings_list.get(&link).unwrap();
        let indices_combined = indices.iter().cloned().collect::<Vec<String>>().join("\0");
        let indices_bytes = indices_combined.as_bytes();

        readings_output_file.write_all(&(indices_bytes.len() as u32).to_le_bytes())?;
        readings_output_file.write_all(&link.to_le_bytes())?;
        readings_output_file.write_all(indices_bytes)?;

        for index in indices {
            key_link_map.insert(index.clone(), offset);
        }

        offset += indices_bytes.len() as u64 + 12;
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
    record_output_path: impl AsRef<Path>,
) -> Result<()> {
    let record_section = &mdict.record_section;
    let new_record_section = MdxRecordSection::from_old_format(record_section);

    let record_output_file = File::create(record_output_path)?;
    let mut record_writer = BufWriter::new(record_output_file);

    new_record_section.write_to(&mut record_writer, &mut mdict.reader)?;
    record_writer.flush()?;

    let never_used = new_record_section.detect_record_indexes_never_used(readings_list);
    println!("Never used record indexes: {:?}", never_used);

    Ok(())
}

pub fn create_fst_index<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    readings_list: &HashMap<u64, HashSet<String>>,
    output_path: impl AsRef<Path>,
    readings_path: impl AsRef<Path>,
    record_output_path: impl AsRef<Path>,
) -> Result<()> {
    let key_link_map = write_readings_data_and_collect_key_offsets(readings_list, readings_path)?;
    write_fst_map(&key_link_map, output_path)?;
    write_record_section(mdict, readings_list, record_output_path)?;

    Ok(())
}
