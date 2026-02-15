
#[cfg(test)]
mod tests {
    use std::fs::File;

    struct TestContext {
        old_header: mdict_tools::header::parser::HeaderInfo,
        old_key_section: mdict_tools::key_index::parser::KeySection,
        new_header: mdict_tools::format::HeaderInfo,
        new_key_section: mdict_tools::format::KeySection,
    }

    fn prepare() -> TestContext {
        let file_path = "resources/jitendex/jitendex.mdx";

        // Old parser (legacy) using FileHandler
        let mut fh = mdict_tools::file_reader::FileHandler::open(file_path).expect("open file for old parser");
        let old_header = mdict_tools::header::parser::HeaderInfo::retrieve_header(&mut fh)
            .expect("old header parse");
        let old_key_section = mdict_tools::key_index::parser::KeySection::retrieve_key_index(&mut fh, &old_header)
            .expect("old key_index parse");

        // New parser (fresh implementation)
        let mut file = File::open(file_path).expect("open file for new parser");
        let new_header = mdict_tools::format::HeaderInfo::read_from(&mut file).expect("new header parse");
        let new_key_section = mdict_tools::format::KeySection::read_from(&mut file, &new_header).expect("new key_index parse");

        TestContext {
            old_header,
            old_key_section,
            new_header,
            new_key_section,
        }
    }

    #[test]
    fn header_parsers_match() {
        let ctx = prepare();

        println!("Old header dict_info: {:?}", ctx.old_header.dict_info());
        println!("Old adler32: {}", ctx.old_header.adler32_checksum());

        // Compare values and print
        for (k, v) in ctx.old_header.dict_info().iter() {
            let new_v = ctx.new_header.get(k);
            println!("Key: {}  old: {}  new: {:?}", k, v, new_v);
            assert!(new_v.is_some(), "missing key '{}' in new header", k);
            assert_eq!(v, new_v.unwrap());
        }

        println!("New header dict_info_size: {}", ctx.new_header.dict_info_size);
        println!("New adler32: {}", ctx.new_header.adler32_checksum);
    }

    #[test]
    fn key_index_parsers_match() {
        let ctx = prepare();

        // Basic parity checks: number of blocks and number of entries should match
        assert_eq!(ctx.old_key_section.num_blocks(), ctx.new_key_section.num_blocks, "num_blocks mismatch");
        assert_eq!(ctx.old_key_section.num_entries(), ctx.new_key_section.num_entries, "num_entries mismatch");

        // If there is at least one block, compare first block's first/last keys
        if ctx.old_key_section.num_blocks() > 0 {
            let old_first = ctx.old_key_section.blocks()[0].first().to_string();
            let old_last = ctx.old_key_section.blocks()[0].last().to_string();

            let new_first = &ctx.new_key_section.key_info_blocks[0].first;
            let new_last = &ctx.new_key_section.key_info_blocks[0].last;

            // Print blocks for debugging parity
            println!("--- Old first block[0] ---");
            println!("num_entries: {}", ctx.old_key_section.blocks()[0].num_entries);
            println!("first: {}", old_first);
            println!("last: {}", old_last);

            println!("--- New first block[0] ---");
            let nb0 = &ctx.new_key_section.key_info_blocks[0];
            println!("num_entries: {}", nb0.num_entries);
            println!("first: {}", nb0.first);
            println!("last: {}", nb0.last);
            println!("compressed_size: {}", nb0.compressed_size);
            println!("decompressed_size: {}", nb0.decompressed_size);

            assert_eq!(old_first, *new_first, "first key mismatch");
            assert_eq!(old_last, *new_last, "last key mismatch");
        }

        // Loop through all blocks and print a short summary for each for manual inspection
        let n = ctx.old_key_section.num_blocks() as usize;
        println!("--- All blocks (count={}) ---", n);
        for i in 0..n {
            let old_b = &ctx.old_key_section.blocks()[i];
            let new_b = &ctx.new_key_section.key_info_blocks[i];
            println!("block {}: old(num_entries={}, first='{}', last='{}') | new(num_entries={}, first='{}', last='{}', comp={}, decomp={})", 
                i,
                old_b.num_entries,
                old_b.first(),
                old_b.last(),
                new_b.num_entries,
                new_b.first,
                new_b.last,
                new_b.compressed_size,
                new_b.decompressed_size
            );
        }
    }

    #[test]
    fn decode_format_block_matches_old_decoder() {
        // Compare the legacy decoder against the new `format::decode_format_block` for a real block
        let ctx = prepare();

        // Only run if there is at least one block
        if ctx.new_key_section.num_blocks == 0 {
            eprintln!("no key blocks to test");
            return;
        }

        use mdict_tools::file_reader::FileHandler;

        // Open file and read the first key-info block raw bytes
        let mut fh = FileHandler::open("resources/jitendex/jitendex.mdx").expect("open file");

        // Use the new key_section metadata to locate the raw compressed block
        let kb0 = &ctx.new_key_section.key_info_blocks[0];
        let prefix = &ctx.new_key_section.key_info_prefix_sum;
        // key blocks follow immediately after the key_info block. Compute
        // the start of the key_blocks area as `next_section_offset - total_key_blocks_size`.
        let total_key_blocks_size = *ctx.new_key_section.key_info_prefix_sum.last().unwrap_or(&0u64);
        let key_blocks_start = ctx.new_key_section.next_section_offset - total_key_blocks_size;
        let offset = key_blocks_start + prefix[0];
        let size = kb0.compressed_size as usize;

        println!("Reading block 0 at offset {} size {}", offset, size);

        let mut buf = vec![0u8; size];
        fh.read_from_file(offset, &mut buf).expect("read raw block");

        // Print raw header bytes and parsed fields for debugging
        if buf.len() >= 8 {
            println!("raw header (first 8 bytes): {:?}", &buf[..8]);
            let enc_le = u32::from_le_bytes(buf[0..4].try_into().unwrap());
            let chk_be = u32::from_be_bytes(buf[4..8].try_into().unwrap());
            println!("parsed header -> encoding (LE) = {} ; checksum (BE) = {}", enc_le, chk_be);
        }

        // Decode with legacy decoder (oracle)
        let old_decoded = mdict_tools::compressed_block::block::decode_block(&buf).expect("old decode");
        let old_adler = minilzo_rs::adler32(&old_decoded);
        println!("legacy decoded len = {} ; legacy adler32 = {}", old_decoded.len(), old_adler);

        // Try decoding with the new format decoder and capture errors to print diagnostics
        match mdict_tools::format::decode_format_block(&buf) {
            Ok(new_decoded) => {
                let new_adler = minilzo_rs::adler32(&new_decoded);
                println!("new decoded len = {} ; new adler32 = {}", new_decoded.len(), new_adler);
                assert_eq!(old_decoded, new_decoded, "decoded outputs must match between old and new decoders");
            }
            Err(e) => {
                eprintln!("new decoder error: {:?}", e);
                // Also compute adler32 of legacy output so we can compare
                eprintln!("legacy adler32 = {} (expected)", old_adler);
                panic!("new decoder failed: {:?}", e);
            }
        }
    }

    #[test]
    fn record_section_parsers_match_old_api() {
        // Compare legacy RecordSection::parse + record_at_offset against
        // new `format::RecordSection::parse` + manual decode using prefix sums.
        let ctx = prepare();

        use std::fs::File;
        use std::io::{Read, Seek, SeekFrom};
        use mdict_tools::file_reader::FileHandler;

        // Open both readers
        let mut fh = FileHandler::open("resources/jitendex/jitendex.mdx").expect("open file for old parser");
        let mut file = File::open("resources/jitendex/jitendex.mdx").expect("open file for new parser");

        // Parse with legacy API
        let mut old_record_section = mdict_tools::records::parser::RecordSection::parse(&ctx.old_header, &ctx.old_key_section, &mut fh);

        // Parse with new format API
        let new_record_section = mdict_tools::format::RecordSection::parse(&ctx.new_header, &ctx.new_key_section, &mut file);

        // Choose a sample offset that should be valid; reuse one used elsewhere in unit tests
        let offset: u64 = 280_887_285;

        // Get the record text via legacy API
        let old_text = old_record_section.record_at_offset(offset, &mut fh);

        // Get the record text via the new API by decoding the compressed block using prefix sums
        let record_index = new_record_section.bin_search_record_index(offset);
        let rec_idx = record_index as usize;

        let start_comp = new_record_section.record_index_prefix_sum[rec_idx].compressed_size;
        let end_comp = new_record_section.record_index_prefix_sum[rec_idx + 1].compressed_size;
        let comp_size = (end_comp - start_comp) as usize;

        // Read compressed bytes from file at record_data_offset + start_comp
        file.seek(SeekFrom::Start(new_record_section.record_data_offset + start_comp)).expect("seek new file");
        let mut comp_buf = vec![0u8; comp_size];
        file.read_exact(&mut comp_buf).expect("read compressed record");

        // Decode using the legacy block decoder for parity
        let decomp = mdict_tools::compressed_block::block::decode_block(&comp_buf).expect("decode record block");

        // Compute decompressed offset inside the decompressed buffer
        let uncompressed_before = new_record_section.record_index_prefix_sum[rec_idx].uncompressed_size;
        let decomp_offset = (offset - uncompressed_before) as usize;

        // Extract bytes until 0x0A 0x00 (same termination used in legacy parser)
        let mut record_bytes = Vec::new();
        for i in decomp_offset..decomp.len() {
            if i + 1 < decomp.len() && decomp[i] == 0x0A && decomp[i + 1] == 0x00 {
                break;
            }
            record_bytes.push(decomp[i]);
        }

        let new_text = std::str::from_utf8(&record_bytes).expect("utf8").to_string();

        // Print debug info to console for inspection
        println!("--- record parity debug ---");
        println!("record_index = {}", record_index);
        println!("compressed_size = {} bytes", comp_size);
        print!("compressed (first up to 64 bytes): ");
        for b in comp_buf.iter().take(std::cmp::min(64, comp_buf.len())) { print!("{:02x} ", b); }
        println!();
        println!("decompressed_len = {} bytes", decomp.len());
        let show_len = std::cmp::min(128, decomp.len());
        println!("decompressed (first {} bytes as UTF-8 lossily): {}", show_len, String::from_utf8_lossy(&decomp[..show_len]));
        println!("old_text = {}", old_text);
        println!("new_text = {}", new_text);
        println!("--- end debug ---");

        // They must match
        assert_eq!(old_text, new_text, "record strings must match between old and new parsers");
    }

    #[test]
    fn mdict_api_basic() {
        use mdict_tools::Mdict;

        // Open the mdx using the new simple API
        let mut md = Mdict::<File>::open("resources/jitendex/jitendex.mdx").expect("open mdict");

        // Search for a short prefix (pick a common Japanese hiragana)
        let prefix = "あ";
        // we'll call `search_keys_prefix` below and handle errors there
        use mdict_tools::file_reader::FileHandler;

        // Prepare legacy/new parsed sections for comparison
        let mut ctx = prepare();

        // Try new API search and collect results for comparison
        let new_results: Vec<(u64, String)> = match md.search_keys_prefix(prefix, 5) {
            Ok(keys) => {
                println!("[new api] search for prefix '{}' returned {} keys", prefix, keys.len());
                let mut v = Vec::new();
                for k in keys.iter() {
                    println!("[new api] key_id={} key='{}'", k.key_id, k.key_text);
                    v.push((k.key_id, k.key_text.clone()));
                }
                v
            }
            Err(e) => {
                panic!("[new api] search keys error: {:?}", e);
            }
        };

        // Use legacy parser API to list first few matching keys from the same file
        let mut fh = FileHandler::open("resources/jitendex/jitendex.mdx").expect("open file for legacy read");

        let mut legacy_results = Vec::new();
        if let Some(mut ptr) = ctx.old_key_section.search_query(prefix, &mut fh) {
            // Iterate using the legacy SearchResultPointer's `next` which returns `KeyBlock`
            while let Some(kb) = ptr.next(&mut fh, &mut ctx.old_key_section) {
                legacy_results.push((kb.key_id, kb.key_text.clone()));
                if legacy_results.len() >= 5 { break; }
            }
        }

        println!("[legacy] found {} keys starting with '{}'", legacy_results.len(), prefix);
        for (id, text) in legacy_results.iter() {
            println!("[legacy] key_id={} key='{}'", id, text);
        }

        // Now assert that the new API results match the legacy results
        assert_eq!(new_results.len(), legacy_results.len(), "result count differs between new and legacy APIs");
        if new_results != legacy_results {
            // Print a helpful diff to aid debugging
            println!("\n--- Result mismatch (new vs legacy) ---");
            for i in 0..std::cmp::max(new_results.len(), legacy_results.len()) {
                let n = new_results.get(i);
                let l = legacy_results.get(i);
                println!("idx {}: new={:?}  legacy={:?}", i, n, l);
            }
        }
        assert_eq!(new_results, legacy_results, "search result tuples (key_id, key_text) must match");

        // Fetch a sample record by an uncompressed offset used in other tests
        let sample_offset: u64 = 280_887_285;
        match md.record_at_uncompressed_offset(sample_offset) {
            Ok(rec) => println!("[new api] record at {}: {}", sample_offset, rec),
            Err(e) => println!("[new api] record_at_uncompressed_offset error: {:?}", e),
        }
    }
}