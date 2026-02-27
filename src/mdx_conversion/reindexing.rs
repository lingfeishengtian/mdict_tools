use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;

use crate::error::Result;
use crate::mdict::Mdict;

pub type ReadingsSet = HashSet<String>;
pub type ReadingsListMap = HashMap<u64, ReadingsSet>;
pub type LinkToKeyIdMap = HashMap<String, u64>;

const LINK_PREFIX: &str = "@@@LINK=";
const PROGRESS_LOG_EVERY: usize = 100_000;

type ReadingsEntry = (u64, String, Option<String>);

fn extract_link(str: &str) -> Option<&str> {
    let remainder = str.strip_prefix(LINK_PREFIX)?;
    let end = remainder
        .find(|c: char| c.is_whitespace())
        .unwrap_or(remainder.len());
    if end == 0 {
        return None;
    }
    Some(&remainder[..end])
}

fn readings_for_key_text(key_text: &str) -> (String, Option<String>) {
    if let Some(left) = key_text.find('【') {
        if let Some(rel_right) = key_text[left + '【'.len_utf8()..].find('】') {
            let right = left + '【'.len_utf8() + rel_right;
            let before = key_text[..left].to_string();
            let inside = key_text[left + '【'.len_utf8()..right].to_string();
            if before == inside {
                return (before, None);
            }
            return (before, Some(inside));
        }
    }

    (key_text.to_string(), None)
}

fn key_id_for_link<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    cached_link_to_key_id: &mut LinkToKeyIdMap,
    link: &str,
) -> Result<u64> {
    if let Some(&key_id) = cached_link_to_key_id.get(link) {
        return Ok(key_id);
    }

    let key_id = mdict.search_keys_prefix(link)?.get(0)?.unwrap().key_id;
    cached_link_to_key_id.insert(link.to_string(), key_id);
    Ok(key_id)
}

fn collect_readings_entries<R: Read + Seek>(mdict: &mut Mdict<R>) -> Result<Vec<ReadingsEntry>> {
    let total_entries = mdict.key_block_index.key_section.num_entries as usize;
    let mut entries = Vec::with_capacity(total_entries);

    for i in 0..total_entries {
        let Some(key_block) = mdict.key_block_index.get(&mut mdict.reader, i)? else {
            break;
        };

        let record = mdict.record_at_index(i)?;
        let link = {
            let record_as_string = String::from_utf8_lossy(&record);
            extract_link(&record_as_string).map(str::to_string)
        };

        entries.push((key_block.key_id, key_block.key_text, link));

        if i % PROGRESS_LOG_EVERY == 0 {
            println!("Processed {} key blocks...", i);
        }
    }

    Ok(entries)
}

fn refresh_direct_link_cache(entries: &[ReadingsEntry]) -> LinkToKeyIdMap {
    entries
        .iter()
        .map(|(key_id, key_text, _link)| (key_text.clone(), *key_id))
        .collect()
}

fn resolve_missing_links<R: Read + Seek>(
    mdict: &mut Mdict<R>,
    cached_link_to_key_id: &mut LinkToKeyIdMap,
    entries: &[ReadingsEntry],
) -> LinkToKeyIdMap {
    let missing_links: HashSet<String> = entries
        .iter()
        .filter_map(|(_key_id, _key_text, link)| link.as_ref())
        .filter(|link| !cached_link_to_key_id.contains_key(link.as_str()))
        .cloned()
        .collect();

    let mut resolved_missing_links = HashMap::new();
    for link in missing_links {
        if let Ok(id) = key_id_for_link(mdict, cached_link_to_key_id, &link) {
            resolved_missing_links.insert(link, id);
        }
    }

    resolved_missing_links
}

fn aggregate_readings_parallel(
    entries: Vec<ReadingsEntry>,
    cached_lookup: Arc<LinkToKeyIdMap>,
    missing_lookup: Arc<LinkToKeyIdMap>,
) -> ReadingsListMap {
    entries
        .into_par_iter()
        .fold(HashMap::new, |mut local_map, (key_id, key_text, link)| {
            let (first_reading, second_reading) = readings_for_key_text(&key_text);
            let cached_key_id = link
                .as_deref()
                .and_then(|link_text| {
                    cached_lookup
                        .get(link_text)
                        .copied()
                        .or_else(|| missing_lookup.get(link_text).copied())
                })
                .unwrap_or(key_id);

            local_map
                .entry(cached_key_id)
                .or_insert_with(HashSet::new)
                .insert(first_reading);

            if let Some(second) = second_reading {
                local_map
                    .entry(cached_key_id)
                    .or_insert_with(HashSet::new)
                    .insert(second);
            }

            local_map
        })
        .reduce(HashMap::new, |mut acc, local_map| {
            for (key_id, keys) in local_map {
                acc.entry(key_id).or_insert_with(HashSet::new).extend(keys);
            }
            acc
        })
}

pub fn build_readings_list_from_path<P: AsRef<Path>>(path: P) -> Result<ReadingsListMap> {
    let file = File::open(path)?;
    let mut mdict = Mdict::new_with_cache(file, usize::MAX)?;
    build_readings_list(&mut mdict)
}

pub fn build_readings_list<R: Read + Seek>(mdict: &mut Mdict<R>) -> Result<ReadingsListMap> {
    let entries = collect_readings_entries(mdict)?;

    let mut cached_link_to_key_id = refresh_direct_link_cache(&entries);

    let resolved_missing_links = resolve_missing_links(mdict, &mut cached_link_to_key_id, &entries);

    let cached_lookup = Arc::new(cached_link_to_key_id);
    let missing_lookup = Arc::new(resolved_missing_links);

    Ok(aggregate_readings_parallel(entries, cached_lookup, missing_lookup))
}

pub fn write_compressed_readings_list<P: AsRef<Path>>(
    readings_list: &ReadingsListMap,
    output_path: P,
) -> Result<()> {
    let mut output_file = File::create(output_path.as_ref())?;
    for (link, readings) in readings_list {
        writeln!(
            output_file,
            "{}: {}",
            link,
            readings.iter().cloned().collect::<Vec<_>>().join(", ")
        )?;
    }
    Ok(())
}

pub fn read_compressed_readings_list<P: AsRef<Path>>(
    input_path: P,
) -> Result<ReadingsListMap> {
    let mut input_file = File::open(input_path.as_ref())?;
    let mut contents = String::new();
    input_file.read_to_string(&mut contents)?;

    let mut readings_list = HashMap::new();

    for line in contents.lines() {
        if let Some((link, readings_str)) = line.split_once(": ") {
                    let readings: ReadingsSet = readings_str.split(", ").map(|s| s.to_string()).collect();
            if let Ok(link_id) = link.parse::<u64>() {
                readings_list.insert(link_id, readings);
            } else {
                eprintln!("Warning: Could not parse link ID from line: {}", line);
            }
        }
    }

    Ok(readings_list)
}
