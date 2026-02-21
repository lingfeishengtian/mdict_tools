use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{MDictError, Result};
use crate::format::{HeaderInfo, KeySection, RecordSection};
use crate::types::{KeyBlock, MdictVersion};

/// Public `Mdict` API using a generic `Read + Seek` reader.
pub struct Mdict<R: Read + Seek> {
    reader: R,
    pub header: HeaderInfo,
    pub key_section: KeySection,
    pub record_section: RecordSection,
}

impl<R: Read + Seek> Mdict<R> {
    /// Create from an arbitrary reader implementing `Read + Seek`.
    /// This will parse the header, key index and record index eagerly.
    pub fn new(mut reader: R) -> Result<Self> {
        let header = HeaderInfo::read_from(&mut reader)?;
        let key_section = KeySection::read_from(&mut reader, &header)?;
        let record_section = RecordSection::parse(&header, &key_section, &mut reader)?;

        Ok(Mdict {
            reader,
            header,
            key_section,
            record_section,
        })
    }

    /// Open a file at `path` and construct an `Mdict<File>`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Mdict<File>> {
        let f = File::open(path).map_err(MDictError::from)?;
        Mdict::new(f)
    }

    /// Search for keys that start with `prefix`. Returns up to `max` results.
    ///
    /// This is a simple implementation that scans matching key blocks and
    /// decodes them on demand. It returns `types::KeyBlock` entries.
    pub fn search_keys_prefix(
        &mut self,
        prefix: &str,
    ) -> Result<impl Iterator<Item = Result<KeyBlock>> + '_> {
        let it = crate::search::iterator_from_prefix(
            &mut self.reader,
            &self.header,
            &self.key_section,
            prefix,
        )?;

        Ok(it)
    }

    /// Retrieve a record given a `KeyBlock`. This finds the next key block
    /// (by key ordering) and treats the difference between the next key's
    /// `key_id` and the provided `key_block.key_id` as the uncompressed
    /// size to read starting at `key_block.key_id`.
    pub fn record_at_key_block(&mut self, key_block: &KeyBlock) -> Result<Vec<u8>> {
        let mut it = crate::search::iterator_from_key(
            &mut self.reader,
            &self.header,
            &self.key_section,
            &key_block.key_text,
        )?;

        let current_key_block = match it.next() {
            Some(Ok(kb)) => kb,
            Some(Err(e)) => return Err(e),
            None => {
                return Err(MDictError::KeyNotFound(format!(
                    "key not found: {}",
                    key_block.key_text
                )))
            }
        };

        let next_key_id = match it.next() {
            Some(Ok(kb)) => Some(kb.key_id),
            Some(Err(e)) => return Err(e),
            None => None,
        };

        let current_key_id = current_key_block.key_id;

        let rec_block = self.record_section.bin_search_record_index(current_key_id) as usize;

        let start_comp = self.record_section.record_index_prefix_sum[rec_block].compressed_size;
        let end_comp = self.record_section.record_index_prefix_sum[rec_block + 1].compressed_size;
        let comp_size = (end_comp - start_comp) as usize;

        let read_offset = self.record_section.record_data_offset + start_comp;
        let mut comp_buf = vec![0u8; comp_size];
        self.reader.seek(SeekFrom::Start(read_offset))?;
        self.reader.read_exact(&mut comp_buf)?;
        let decomp = crate::format::decode_format_block(&comp_buf)?;

        let uncompressed_before =
            self.record_section.record_index_prefix_sum[rec_block].uncompressed_size;
        let decomp_offset = (current_key_id - uncompressed_before) as usize;

        let bytes_available = decomp.len().saturating_sub(decomp_offset);
        let bytes_to_take = match next_key_id {
            Some(nk) => ((nk - current_key_id) as usize).min(bytes_available),
            None => bytes_available,
        };

        let end = decomp_offset
            .saturating_add(bytes_to_take)
            .min(decomp.len());

        let slice = &decomp[decomp_offset..end];

        if self.header.get_version() != MdictVersion::MDD && slice.ends_with(&[0x0A, 0x00]) {
            return Ok(Vec::from(&slice[..slice.len() - 2]))
        }

        Ok(Vec::from(slice))
    }
}
