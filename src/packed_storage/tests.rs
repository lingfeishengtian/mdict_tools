#[cfg(test)]
mod packed_storage_tests {
    use std::env;
    use std::fs::{create_dir_all, File};
    use std::io::{Cursor, Seek, SeekFrom};
    use std::path::PathBuf;

    use super::super::{CompressionEncoding, PackedStorageIndex, PackedStorageWriter};

    fn entries() -> Vec<Vec<u8>> {
        vec![b"aaaa".to_vec(), b"bbbb".to_vec(), b"cccc".to_vec()]
    }

    fn test_output_dir() -> PathBuf {
        let base = env::var("TEST_OUTPUT_DIR")
            .or_else(|_| env::var("MDICT_TEST_OUTPUT_DIR"))
            .unwrap_or_else(|_| "test_output".to_string());
        PathBuf::from(base).join("packed_storage")
    }

    fn write_entries_to_writer(
        encoding: CompressionEncoding,
        block_size: usize,
        values: &[Vec<u8>],
    ) -> (PackedStorageWriter, Vec<u64>) {
        let mut writer = PackedStorageWriter::new(encoding, 10, block_size).unwrap();
        let mut offsets = Vec::with_capacity(values.len());
        for value in values {
            offsets.push(writer.push_entry(value).unwrap());
        }
        (writer, offsets)
    }

    fn assert_roundtrip_entries(
        storage: &[u8],
        offsets: &[u64],
        values: &[Vec<u8>],
        expected_blocks_at_least: usize,
    ) {
        let mut cursor = Cursor::new(storage.to_vec());
        let index = PackedStorageIndex::parse_from_reader(&mut cursor).unwrap();
        assert_eq!(index.header.num_entries, values.len() as u64);
        assert!(index.header.block_prefix_sum.len() >= expected_blocks_at_least);

        for (i, expected) in values.iter().enumerate() {
            cursor.seek(SeekFrom::Start(0)).unwrap();
            let actual = index
                .read_from_offset_with_options(&mut cursor, offsets[i], None, Some(expected.len() as u64))
                .unwrap();
            assert_eq!(&actual, expected);
        }
    }

    #[test]
    fn packed_storage_empty_round_trip() {
        let writer = PackedStorageWriter::new(CompressionEncoding::Raw, 0, 64).unwrap();
        let bytes = writer.finish_into_bytes().unwrap();

        let mut cursor = Cursor::new(bytes);
        let index = PackedStorageIndex::parse_from_reader(&mut cursor).unwrap();
        assert_eq!(index.header.num_entries, 0);
        assert_eq!(index.header.block_prefix_sum.len(), 1);
        assert_eq!(index.total_uncompressed_size(), Some(0));
    }

    #[test]
    fn packed_storage_single_block_round_trip() {
        let entries = vec![b"abc".to_vec(), b"defghi".to_vec()];
        let (writer, offsets) = write_entries_to_writer(CompressionEncoding::Raw, 1024, &entries);
        let bytes = writer.finish_into_bytes().unwrap();

        assert_eq!(offsets, vec![0, 3]);
        assert_roundtrip_entries(&bytes, &offsets, &entries, 2);
    }

    #[test]
    fn packed_storage_multiple_blocks_round_trip() {
        let entries = entries();
        let (writer, offsets) = write_entries_to_writer(CompressionEncoding::Zstd, 8, &entries);
        let bytes = writer.finish_into_bytes().unwrap();

        assert_roundtrip_entries(&bytes, &offsets, &entries, 3);
    }

    #[test]
    fn packed_storage_read_with_terminator_option() {
        let mut writer = PackedStorageWriter::new(CompressionEncoding::Raw, 0, 4).unwrap();
        writer.push_entry(b"abc").unwrap();
        writer.push_entry(&[0x0A, 0x00]).unwrap();
        writer.push_entry(b"tail").unwrap();
        let bytes = writer.finish_into_bytes().unwrap();

        let mut cursor = Cursor::new(bytes);
        let index = PackedStorageIndex::parse_from_reader(&mut cursor).unwrap();
        cursor.seek(SeekFrom::Start(0)).unwrap();

        let result = index
            .read_from_offset_with_options(&mut cursor, 0, Some(&[0x0A, 0x00]), None)
            .unwrap();

        assert_eq!(result, b"abc");
    }

    #[test]
    fn packed_storage_read_with_record_size_option() {
        let mut writer = PackedStorageWriter::new(CompressionEncoding::Raw, 0, 3).unwrap();
        writer.push_entry(b"ab").unwrap();
        writer.push_entry(b"cd").unwrap();
        writer.push_entry(b"ef").unwrap();
        let bytes = writer.finish_into_bytes().unwrap();

        let mut cursor = Cursor::new(bytes);
        let index = PackedStorageIndex::parse_from_reader(&mut cursor).unwrap();
        cursor.seek(SeekFrom::Start(0)).unwrap();

        let result = index
            .read_from_offset_with_options(&mut cursor, 1, None, Some(4))
            .unwrap();

        assert_eq!(result, b"bcde");
    }

    #[test]
    fn packed_storage_write_file_and_reread_entries() {
        let output_dir = test_output_dir();
        create_dir_all(&output_dir).unwrap();
        let output_path = output_dir.join("packed_storage_roundtrip.bin");

        let entries = entries();
        let (writer, offsets) = write_entries_to_writer(CompressionEncoding::Zstd, 8, &entries);

        let mut out_file = File::create(&output_path).unwrap();
        writer.finish_to_writer(&mut out_file).unwrap();
        drop(out_file);

        let mut in_file = File::open(&output_path).unwrap();
        let index = PackedStorageIndex::parse_from_reader(&mut in_file).unwrap();
        assert_eq!(index.header.num_entries, entries.len() as u64);

        for (i, expected) in entries.iter().enumerate() {
            in_file.seek(SeekFrom::Start(0)).unwrap();
            let actual = index
                .read_from_offset_with_options(
                    &mut in_file,
                    offsets[i],
                    None,
                    Some(expected.len() as u64),
                )
                .unwrap();
            assert_eq!(&actual, expected);
        }
    }
}
