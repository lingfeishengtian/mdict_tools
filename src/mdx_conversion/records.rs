use std::{collections::{HashMap, HashSet}, io::{Read, Seek, SeekFrom, Write}};

use binrw::{BinRead, BinWrite};

use crate::error::{MDictError, Result};

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
}
