use std::io::{Read, Seek, SeekFrom};
use std::fs::File;
use std::path::Path;

use crate::error::{Result, MDictError};
use crate::types::KeyBlock;
use crate::format::{HeaderInfo, KeySection, RecordSection};

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
        // Parse header and sections using format::* helpers
        let header = HeaderInfo::read_from(&mut reader)?;
        let key_section = KeySection::read_from(&mut reader, &header)?;
        let record_section = RecordSection::parse(&header, &key_section, &mut reader);

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
    pub fn search_keys_prefix(&mut self, prefix: &str, max: usize) -> Result<Vec<KeyBlock>> {
        let mut out = Vec::new();
        // Compute once: key blocks area starts at `next_section_offset - total_key_blocks_size`.
        let total_key_blocks_size = *self.key_section.key_info_prefix_sum.last().unwrap_or(&0u64);
        let key_blocks_start = self.key_section.next_section_offset - total_key_blocks_size;

        // Binary search for the first block that might contain the prefix
        let blocks = &self.key_section.key_info_blocks;
        let n = blocks.len();
        let start_idx = blocks.partition_point(|b| b.last.as_str() < prefix);

        for i in start_idx..n {
            let kb = &blocks[i];

            // Early termination: if this block's first key is greater than the
            // prefix and it doesn't start with the prefix then no subsequent
            // block can contain matching keys (blocks ordered by first key).
            if kb.first.as_str() > prefix && !kb.first.starts_with(prefix) {
                break;
            }

            // cheap filter using block first/last (inclusive range check)
            if !(kb.first.starts_with(prefix)
                || kb.last.starts_with(prefix)
                || (kb.first.as_str() <= prefix && kb.last.as_str() >= prefix))
            {
                continue;
            }

            // read and decode the key block
            let offset = key_blocks_start + self.key_section.key_info_prefix_sum[i];
            let size = kb.compressed_size as usize;
            let mut buf = vec![0u8; size];
            self.reader.seek(SeekFrom::Start(offset))?;
            self.reader.read_exact(&mut buf)?;

            // decode using format decoder (matches on-disk format)
            let decoded = crate::format::decode_format_block(&buf)?;

            // parse entries: key_id (big-endian u64) followed by null-terminated key bytes
            let mut off = 0usize;
            while off + 8 <= decoded.len() {
                let key_id = u64::from_be_bytes(decoded[off..off+8].try_into().unwrap());
                off += 8;

                // read until NUL (or end)
                let start = off;
                while off < decoded.len() && decoded[off] != 0 { off += 1; }
                let key_text = String::from_utf8_lossy(&decoded[start..off]).to_string();
                // advance past NUL if present
                if off < decoded.len() && decoded[off] == 0 { off += 1; }

                // only include keys that actually start with the requested prefix
                if !key_text.starts_with(prefix) { continue; }

                out.push(KeyBlock { key_id, key_text });
                if out.len() >= max { return Ok(out); }
            }
        }

        Ok(out)
    }

    /// Retrieve a record string by an uncompressed (logical) offset into
    /// the record data area. This mirrors how the record parsing test
    /// extracts a record given an uncompressed offset.
    pub fn record_at_uncompressed_offset(&mut self, offset_uncompressed: u64) -> Result<String> {
        // Find which record block contains the uncompressed offset
        let rec_block = self.record_section.bin_search_record_index(offset_uncompressed) as usize;

        let start_comp = self.record_section.record_index_prefix_sum[rec_block].compressed_size;
        let end_comp = self.record_section.record_index_prefix_sum[rec_block + 1].compressed_size;
        let comp_size = (end_comp - start_comp) as usize;

        // Read compressed bytes
        let read_offset = self.record_section.record_data_offset + start_comp;
        let mut comp_buf = vec![0u8; comp_size];
        self.reader.seek(SeekFrom::Start(read_offset))?;
        self.reader.read_exact(&mut comp_buf)?;

        // Decode compressed block using format decoder
        let decomp = crate::format::decode_format_block(&comp_buf)?;

        // Compute offset inside decompressed buffer
        let uncompressed_before = self.record_section.record_index_prefix_sum[rec_block].uncompressed_size;
        let decomp_offset = (offset_uncompressed - uncompressed_before) as usize;

        // Extract up to terminator 0x0A 0x00 (legacy terminator)
        let mut record_bytes = Vec::new();
        for i in decomp_offset..decomp.len() {
            if i + 1 < decomp.len() && decomp[i] == 0x0A && decomp[i + 1] == 0x00 { break; }
            record_bytes.push(decomp[i]);
        }

        let s = String::from_utf8(record_bytes).map_err(|e| MDictError::from(format!("invalid utf8: {}", e)))?;
        Ok(s)
    }
}