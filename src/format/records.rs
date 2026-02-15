use crate::file_reader::FileHandler;
use crate::format::{HeaderInfo, KeySection};
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use binrw::BinRead;

pub struct RecordSection {
    pub record_data_offset: u64,
    pub record_index_prefix_sum: Vec<RecordIndex>,
}

#[derive(Clone, Debug)]
pub struct RecordIndex {
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}

#[derive(BinRead, Debug)]
#[br(big)]
struct RecordHeaderV1 {
    num_record_blocks: u32,
    num_entries: u32,
    byte_size_record_index: u32,
    byte_size_record_data: u32,
}

#[derive(BinRead, Debug)]
#[br(big)]
struct RecordPairV1 {
    compressed_size: u32,
    uncompressed_size: u32,
}

#[derive(BinRead, Debug)]
#[br(big)]
struct RecordHeaderV2 {
    num_record_blocks: u64,
    num_entries: u64,
    byte_size_record_index: u64,
    byte_size_record_data: u64,
}

#[derive(BinRead, Debug)]
#[br(big)]
struct RecordPairV2 {
    compressed_size: u64,
    uncompressed_size: u64,
}

impl RecordSection {
    /// Parse the record index using `FileHandler` and `binrw` for the on-disk formats.
    /// Leaves `record_data_offset` pointing at the start of the record data area.
    pub fn parse<R: Read + Seek>(header_index: &HeaderInfo, key_index: &KeySection, reader: &mut R) -> RecordSection {
        // offset starts at the section immediately following the key blocks
        let mut offset = key_index.next_section_offset;

        let read_size = match header_index.get_version() {
            crate::header::parser::MdictVersion::V1 => 4usize,
            crate::header::parser::MdictVersion::V2 => 8usize,
            crate::header::parser::MdictVersion::V3 => panic!("Unsupported version for records"),
        };

        // Read header (num blocks, entries, byte sizes)
        let mut header_buf = vec![0u8; read_size * 4];
        // seek + read_exact into buffer
        reader.seek(SeekFrom::Start(offset)).unwrap();
        reader.read_exact(&mut header_buf).unwrap();
        offset += header_buf.len() as u64;

        let mut record_index = Vec::new();

        if read_size == 4 {
            let header: RecordHeaderV1 = RecordHeaderV1::read(&mut Cursor::new(&header_buf)).unwrap();
            let num_blocks = header.num_record_blocks as usize;
            let byte_size_record_index = header.byte_size_record_index as usize;

            // Read the index blob containing `num_blocks` pairs
            let mut index_buf = vec![0u8; byte_size_record_index];
            reader.seek(SeekFrom::Start(offset)).unwrap();
            reader.read_exact(&mut index_buf).unwrap();
            offset += index_buf.len() as u64;

            let mut cur = Cursor::new(&index_buf);
            for _ in 0..num_blocks {
                let pair: RecordPairV1 = RecordPairV1::read(&mut cur).unwrap();
                let last = record_index.last().cloned().unwrap_or(RecordIndex { compressed_size: 0, uncompressed_size: 0 });
                record_index.push(RecordIndex {
                    compressed_size: last.compressed_size + pair.compressed_size as u64,
                    uncompressed_size: last.uncompressed_size + pair.uncompressed_size as u64,
                });
            }
        } else {
            let header: RecordHeaderV2 = RecordHeaderV2::read(&mut Cursor::new(&header_buf)).unwrap();
            let num_blocks = header.num_record_blocks as usize;
            let byte_size_record_index = header.byte_size_record_index as usize;

            let mut index_buf = vec![0u8; byte_size_record_index];
            reader.seek(SeekFrom::Start(offset)).unwrap();
            reader.read_exact(&mut index_buf).unwrap();
            offset += index_buf.len() as u64;

            let mut cur = Cursor::new(&index_buf);
            for _ in 0..num_blocks {
                let pair: RecordPairV2 = RecordPairV2::read(&mut cur).unwrap();
                let last = record_index.last().cloned().unwrap_or(RecordIndex { compressed_size: 0, uncompressed_size: 0 });
                record_index.push(RecordIndex {
                    compressed_size: last.compressed_size + pair.compressed_size,
                    uncompressed_size: last.uncompressed_size + pair.uncompressed_size,
                });
            }
        }

        // Prepend zero entry to match previous prefix-sum shape
        let mut prefix = Vec::with_capacity(record_index.len() + 1);
        prefix.push(RecordIndex { compressed_size: 0, uncompressed_size: 0 });
        prefix.extend(record_index);

        RecordSection {
            record_data_offset: offset,
            record_index_prefix_sum: prefix,
        }
    }

    /// Binary-search for the record index containing `offset` (uncompressed offset)
    pub fn bin_search_record_index(&self, offset: u64) -> u64 {
        // Use the standard slice helper `partition_point` to find the first
        // index where `uncompressed_size > offset`, then subtract one to get
        // the record block that contains `offset`.
        // This avoids hand-rolling the binary search loop.
        let idx = self.record_index_prefix_sum.partition_point(|ri| ri.uncompressed_size <= offset);
        // `idx` is at least 1 because we prepend a zero prefix entry during parse.
        (idx - 1) as u64
    }
}
