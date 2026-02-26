use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{MDictError, Result};
use crate::format::{key_block, HeaderInfo, KeySection, RecordSection};
use crate::mdict::Mdict;
use crate::prefix_key_block_index::PrefixKeyBlockIndex;

/// Re-indexing functionality for MDX files to convert to MDIC format
pub struct MdxReindexer<R: Read + Seek> {
    /// The original Mdict file that we're reindexing
    pub mdict: Mdict<R>,
    /// ReadingsList compiled from Map<LINK, [Search Index]>
    pub readings_list: HashMap<u64, HashSet<String>>,
    pub cached_link_to_key_id: HashMap<String, u64>,
}

const LINK_REGEX: &str = r"@@@LINK=([^\s]+)";

impl<R: Read + Seek> MdxReindexer<R> {
    /// Create a new reindexer from an MDX file
    pub fn new(mdict: Mdict<R>) -> Self {
        Self {
            mdict,
            readings_list: HashMap::new(),
            cached_link_to_key_id: HashMap::new(),
        }
    }

    pub fn extract_link(str: &str) -> Option<&str> {
        let re = regex::Regex::new(LINK_REGEX).unwrap();
        re.captures(str)
            .and_then(|caps| caps.get(1).map(|m| m.as_str()))
    }

    pub fn key_id_for_link(&mut self, link: &str) -> Result<u64> {
        if let Some(&key_id) = self.cached_link_to_key_id.get(link) {
            return Ok(key_id);
        }

        let key_id = self.mdict.search_keys_prefix(link)?.get(0)?.unwrap().key_id;

        self.cached_link_to_key_id.insert(link.to_string(), key_id);
        Ok(key_id)
    }

    pub fn build_readings_list(&mut self) -> Result<()> {
        let mut i = 0;

        while let Ok(Some(key_block)) = self.mdict.key_block_index.get(&mut self.mdict.reader, i) {
            let keys_set = if key_block.key_text.contains("【") && key_block.key_text.contains("】")
            {
                // capture the text inside the brackets, pair it with the text before the brackets, and add both to the set
                let re = regex::Regex::new(r"^(.*?)【(.*?)】").unwrap();
                if let Some(caps) = re.captures(&key_block.key_text) {
                    let before_brackets = caps.get(1).map_or("", |m| m.as_str());
                    let inside_brackets = caps.get(2).map_or("", |m| m.as_str());
                    let mut set = HashSet::new();
                    set.insert(before_brackets.to_string());
                    set.insert(inside_brackets.to_string());
                    set
                } else {
                    let mut set = HashSet::new();
                    set.insert(key_block.key_text.clone());
                    set
                }
            } else {
                let mut set = HashSet::new();
                set.insert(key_block.key_text.clone());
                set
            };

            let record = self.mdict.record_at_index(i)?;

            let record_as_string = String::from_utf8_lossy(&record);

            let key_id = if let Some(link) = Self::extract_link(&record_as_string) {
                self.key_id_for_link(link)?
            } else {
                self.cached_link_to_key_id
                    .insert(key_block.key_text, key_block.key_id);
                key_block.key_id
            };

            self.readings_list
                .entry(key_id)
                .or_insert_with(HashSet::new)
                .extend(keys_set);

            if i % 1000 == 0 {
                println!("Processed {} key blocks...", i);
            }

            i += 1;
        }

        Ok(())
    }

    pub fn get_readings_list(&self) -> &HashMap<u64, HashSet<String>> {
        &self.readings_list
    }

    pub fn write_compressed_readings_list<P: AsRef<Path>>(&self, output_path: P) -> Result<()> {
        let mut output_file = File::create(output_path.as_ref())?;
        for (link, readings) in &self.readings_list {
            writeln!(
                output_file,
                "{}: {}",
                link,
                readings.iter().cloned().collect::<Vec<_>>().join(", ")
            )?;
        }
        Ok(())
    }

    /// Create a new MDIC file with updated indexing
    pub fn create_mdic_file<P: AsRef<Path>>(&self, output_path: P) -> Result<()> {
        // This would create the final MDIC format (MDI + MDC)
        // For now, we'll just demonstrate the structure

        let mut output_file = File::create(output_path.as_ref())?;

        // Write basic file structure
        writeln!(output_file, "MDIC Conversion from MDX")?;
        writeln!(output_file, "Original file: {:?}", output_path.as_ref())?;
        writeln!(
            output_file,
            "Total links processed: {}",
            self.readings_list.len()
        )?;

        Ok(())
    }
}

pub fn read_compressed_readings_list<P: AsRef<Path>>(
    input_path: P,
) -> Result<HashMap<u64, HashSet<String>>> {
    let mut input_file = File::open(input_path.as_ref())?;
    let mut contents = String::new();
    input_file.read_to_string(&mut contents)?;

    let mut readings_list = HashMap::new();

    for line in contents.lines() {
        if let Some((link, readings_str)) = line.split_once(": ") {
            let readings: HashSet<String> =
                readings_str.split(", ").map(|s| s.to_string()).collect();
            if let Ok(link_id) = link.parse::<u64>() {
                readings_list.insert(link_id, readings);
            } else {
                eprintln!("Warning: Could not parse link ID from line: {}", line);
            }
        }
    }

    Ok(readings_list)
}
