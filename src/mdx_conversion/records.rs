use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom, Write},
};

use crate::error::{MDictError, Result};
use crate::packed_storage::{CompressionEncoding, PackedStorageIndex, PackedStorageWriter};
use crate::Mdict;

const RECORDS_ZSTD_LEVEL: u8 = 10;
const TARGET_UNCOMPRESSED_BLOCK_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct RecordSection {
    storage_index: PackedStorageIndex,
}

impl RecordSection {
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<RecordSection> {
        reader.seek(SeekFrom::Start(0))?;
        let storage_index = PackedStorageIndex::parse_from_reader(reader)?;
        Ok(RecordSection { storage_index })
    }

    pub fn decode_record<R: Read + Seek>(
        &self,
        reader: &mut R,
        link: u64,
        record_size: Option<u64>,
    ) -> Result<Vec<u8>> {
        let terminator = if record_size.is_none() {
            Some(&[0x0A, 0x00][..])
        } else {
            None
        };

        self.storage_index.read_from_offset_with_options(
            reader,
            link,
            terminator,
            record_size,
        )
    }

    pub fn rebuild_compacted_zstd_from_mdict<R: Read + Seek, W: Write + Seek>(
        mdict: &mut Mdict<R>,
        readings_list: &HashMap<u64, HashSet<String>>,
        ordered_old_links: &[u64],
        writer: &mut W,
    ) -> Result<HashMap<u64, u64>> {
        let total_entries = mdict.key_block_index.key_section.num_entries as usize;
        let mut key_id_to_index = HashMap::with_capacity(total_entries);

        for index in 0..total_entries {
            let Some(key_block) = mdict.key_block_index.get(&mut mdict.reader, index)? else {
                break;
            };
            key_id_to_index.insert(key_block.key_id, index);
        }

        let mut seen = HashSet::new();
        let mut storage_writer = PackedStorageWriter::new(
            CompressionEncoding::Zstd,
            RECORDS_ZSTD_LEVEL,
            TARGET_UNCOMPRESSED_BLOCK_SIZE,
        )?;
        let mut link_remap = HashMap::new();

        for &old_link in ordered_old_links {
            if !readings_list.contains_key(&old_link) || !seen.insert(old_link) {
                continue;
            }

            let index = *key_id_to_index.get(&old_link).ok_or_else(|| {
                MDictError::InvalidArgument(format!("missing key index for link {}", old_link))
            })?;

            let record = mdict.record_at_index(index)?;
            let new_link = storage_writer.push_entry(&record)?;
            link_remap.insert(old_link, new_link);
        }

        if link_remap.is_empty() {
            return Err(MDictError::InvalidArgument(
                "no referenced records found for compaction".to_string(),
            ));
        }

        storage_writer.finish_to_writer(writer)?;
        Ok(link_remap)
    }
}
