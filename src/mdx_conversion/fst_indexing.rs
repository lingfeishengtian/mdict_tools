use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, Write};
use std::path::Path;

use fst::MapBuilder;
use crate::error::Result;
use crate::mdx_conversion::records::RecordSection as MdxRecordSection;
use crate::Mdict;

pub fn write_readings_data_and_collect_key_offsets(
    readings_list: &HashMap<u64, HashSet<String>>,
    link_order: &[u64],
    link_remap: &HashMap<u64, u64>,
    readings_path: impl AsRef<Path>,
) -> Result<HashMap<String, u64>> {
    crate::mdx_conversion::readings::write_readings_data_and_collect_key_offsets(
        readings_list,
        link_order,
        link_remap,
        readings_path,
    )
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
