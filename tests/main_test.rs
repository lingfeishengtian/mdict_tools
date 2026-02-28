#[cfg(test)]
mod tests {
    use fst::Streamer;
    use mdict_tools::mdx_conversion::fst_map::FSTMap;
    use mdict_tools::types::KeyBlock;
    use mdict_tools::{format, mdx_conversion, Mdict};
    use std::fs::{create_dir_all, File};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::Instant;
    use std::usize;
    use sysinfo::{get_current_pid, ProcessesToUpdate, System};

    const SAMPLE_PATH: &str = "resources/jitendex/jitendex.mdx";
    const SAMPLE_MDD_PATH: &str = "resources/jitendex/jitendex.mdd";

    fn get_record_for_key_id(md: &mut Mdict<File>, key_block: &KeyBlock) -> Vec<u8> {
        let rec = md
            .record_at_key_block(key_block)
            .unwrap_or_else(|_| Vec::new());
        if let Some(suffix) = rec.strip_prefix(b"@@@LINK=") {
            if let Ok(tag) = std::str::from_utf8(suffix) {
                println!(
                    "Found link record for key_id {}: tag='{}'",
                    key_block.key_id, tag
                );
                let key_index: Vec<_> = match md.search_keys_prefix(tag) {
                    Ok(mut it) => it.collect_to_vec().unwrap_or_else(|_| vec![]),
                    Err(_) => vec![],
                };

                if let Some(k) = key_index.first() {
                    println!(
                        "Found key block for link tag '{}': key_id={} key_text='{}'",
                        tag, k.key_id, k.key_text
                    );
                    return get_record_for_key_id(md, k);
                }
            }
            return Vec::new();
        }
        return rec;
    }

    #[test]
    fn print_new_header_and_key_index() {
        let mut file = File::open(SAMPLE_PATH).expect("open mdx file");
        let header = mdict_tools::format::HeaderInfo::read_from(&mut file).expect("read header");

        println!("New header dict_info_size = {}", header.dict_info_size);
        println!("New header adler32 = {}", header.adler32_checksum);
        println!("New header entries:");
        for (k, v) in header.dict_info.iter() {
            println!("  {} => {}", k, v);
        }

        let mut file = File::open(SAMPLE_PATH).expect("open mdx file");
        let key_section = mdict_tools::format::KeySection::read_from(&mut file, &header)
            .expect("read key section");

        println!(
            "KeySection: num_blocks = {} num_entries = {}",
            key_section.num_blocks, key_section.num_entries
        );
        if !key_section.key_info_blocks.is_empty() {
            let kb0 = &key_section.key_info_blocks[0];
            println!(
                "First key block[0]: num_entries={} first='{}' last='{}' comp={} decomp={}",
                kb0.num_entries, kb0.first, kb0.last, kb0.compressed_size, kb0.decompressed_size
            );
        }

        let limit = std::cmp::min(8usize, key_section.key_info_blocks.len());
        println!("Printing {} key blocks summaries:", limit);
        for (i, kb) in key_section.key_info_blocks.iter().take(limit).enumerate() {
            println!(
                "block {}: num_entries={} first='{}' last='{}' comp={} decomp={}",
                i, kb.num_entries, kb.first, kb.last, kb.compressed_size, kb.decompressed_size
            );
        }
    }

    #[test]
    fn decode_first_key_block_and_print() {
        let mut file = File::open(SAMPLE_PATH).expect("open mdx file");
        let header = mdict_tools::format::HeaderInfo::read_from(&mut file).expect("read header");

        let mut file = File::open(SAMPLE_PATH).expect("open mdx file");
        let key_section = mdict_tools::format::KeySection::read_from(&mut file, &header)
            .expect("read key section");

        if key_section.key_info_blocks.is_empty() {
            eprintln!("no key blocks found");
            return;
        }

        let total_key_blocks_size = *key_section.key_info_prefix_sum.last().unwrap_or(&0u64);
        let key_blocks_start = key_section.next_section_offset - total_key_blocks_size;
        let offset = key_blocks_start + key_section.key_info_prefix_sum[0];
        let size = key_section.key_info_blocks[0].compressed_size as usize;

        println!("Reading block 0 at offset {} size {}", offset, size);

        let mut file = File::open(SAMPLE_PATH).expect("open mdx file");
        file.seek(SeekFrom::Start(offset)).expect("seek to block");
        let mut buf = vec![0u8; size];
        file.read_exact(&mut buf).expect("read block bytes");

        if buf.len() >= 8 {
            println!("raw header (first 8 bytes): {:02x?}", &buf[..8]);
            let enc_le = u32::from_le_bytes(buf[0..4].try_into().unwrap());
            let chk_be = u32::from_be_bytes(buf[4..8].try_into().unwrap());
            println!(
                "parsed header -> encoding (LE) = {} ; checksum (BE) = {}",
                enc_le, chk_be
            );
        }

        match format::decode_format_block(&buf) {
            Ok(decompressed) => {
                let adler = minilzo_rs::adler32(&decompressed);
                println!("decoded len = {} ; adler32 = {}", decompressed.len(), adler);
                println!(
                    "first 128 bytes of decompressed (lossy): {}",
                    String::from_utf8_lossy(
                        &decompressed[..std::cmp::min(128, decompressed.len())]
                    )
                );
            }
            Err(e) => {
                eprintln!("decoder error: {:?}", e);
                panic!("decoder failed");
            }
        }
    }

    #[test]
    fn search_for_jisho() {
        let f = File::open(SAMPLE_PATH).expect("open mdx file");

        let mut sys = System::new();
        let pid = get_current_pid().expect("get pid");
        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mem_before = sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let virt_before = sys.process(pid).map(|p| p.virtual_memory()).unwrap_or(0);

        let mut md = mdict_tools::Mdict::new(f).expect("open mdx via Mdict");

        let prefix = "辞";
        println!("[new api] searching for prefix '{}', max 10", prefix);

        let start = Instant::now();
        let mut iter = md.search_keys_prefix(prefix).expect("search");
        // let res = iter.collect_to_vec().expect("collect results");

        println!("[len] {}", iter.len());
        let elapsed = start.elapsed();

        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mem_after = sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let virt_after = sys.process(pid).map(|p| p.virtual_memory()).unwrap_or(0);

        let len = iter.len();
        let is_empty = iter.is_empty();

        println!("[new api] found {} keys starting with '{}'", len, prefix);
        let mem_mb_before = (mem_before as f64) / 1024.0 / 1024.0;
        let mem_mb_after = (mem_after as f64) / 1024.0 / 1024.0;
        let mem_mb_delta = mem_mb_after - mem_mb_before;

        let virt_mb_before = (virt_before as f64) / 1024.0 / 1024.0;
        let virt_mb_after = (virt_after as f64) / 1024.0 / 1024.0;
        let virt_mb_delta = virt_mb_after - virt_mb_before;

        println!("[metrics] elapsed_secs = {:.6}", elapsed.as_secs_f64());
        println!("[metrics] rss_before = {:.2} MB", mem_mb_before);
        println!("[metrics] rss_after  = {:.2} MB", mem_mb_after);
        println!("[metrics] rss_delta  = {:.2} MB", mem_mb_delta);
        println!("[metrics] virt_before = {:.2} MB", virt_mb_before);
        println!("[metrics] virt_after  = {:.2} MB", virt_mb_after);
        println!("[metrics] virt_delta  = {:.2} MB", virt_mb_delta);
        for kb in iter.take(100).unwrap_or(Vec::new()) {
            println!("[new api] key_id={} key='{}'", kb.key_id, kb.key_text);
            let rec_bytes = get_record_for_key_id(&mut md, &kb);
            println!(
                "[new api] record for key_id {} record_size: {}: {}",
                kb.key_id,
                rec_bytes.len(),
                String::from_utf8_lossy(&rec_bytes)
            );
        }

        assert!(
            !is_empty,
            "expected at least one key for prefix '{}'",
            prefix
        );
    }

    #[test]
    fn list_some_mdd_keys() {
        let mut file = File::open(SAMPLE_MDD_PATH).expect("open mdx file");
        let header = mdict_tools::format::HeaderInfo::read_from(&mut file).expect("read header");

        println!("New header dict_info_size = {}", header.dict_info_size);
        println!("New header adler32 = {}", header.adler32_checksum);
        println!("New header entries:");
        for (k, v) in header.dict_info.iter() {
            println!("  {} => {}", k, v);
        }

        let mut file = File::open(SAMPLE_MDD_PATH).expect("open mdx file");
        let key_section = mdict_tools::format::KeySection::read_from(&mut file, &header)
            .expect("read key section");

        println!(
            "KeySection: num_blocks = {} num_entries = {}",
            key_section.num_blocks, key_section.num_entries
        );
        if !key_section.key_info_blocks.is_empty() {
            let kb0 = &key_section.key_info_blocks[0];
            println!(
                "First key block[0]: num_entries={} first='{}' last='{}' comp={} decomp={}",
                kb0.num_entries, kb0.first, kb0.last, kb0.compressed_size, kb0.decompressed_size
            );
        }

        let limit = std::cmp::min(8usize, key_section.key_info_blocks.len());
        println!("Printing {} key blocks summaries:", limit);
        for (i, kb) in key_section.key_info_blocks.iter().take(limit).enumerate() {
            println!(
                "block {}: num_entries={} first='{}' last='{}' comp={} decomp={}",
                i, kb.num_entries, kb.first, kb.last, kb.compressed_size, kb.decompressed_size
            );
        }
    }

    #[test]
    fn write_mdd_record_to_output() {
        let f = File::open(SAMPLE_MDD_PATH).expect("open mdd file");
        let mut md = mdict_tools::Mdict::new(f).expect("open mdd via Mdict");

        let mut hf = File::open(SAMPLE_MDD_PATH).expect("open mdd file for header");
        let header = mdict_tools::format::HeaderInfo::read_from(&mut hf).expect("read header");
        let mut kf = File::open(SAMPLE_MDD_PATH).expect("open mdd file for keys");
        let key_section =
            mdict_tools::format::KeySection::read_from(&mut kf, &header).expect("read key section");

        if key_section.key_info_blocks.is_empty() {
            panic!("no key blocks found in MDD");
        }

        let first_key = key_section.key_info_blocks[1].first.clone();
        let matches: Vec<_> = md
            .search_keys_prefix(&first_key)
            .expect("search mdd")
            .collect_to_vec()
            .expect("collect results");
        if matches.is_empty() {
            panic!("no matching key for first key text");
        }

        println!(
            "Found matching key_id {} for first key text '{}'",
            matches[0].key_id, first_key
        );

        let keyblock = matches[0].clone();

        let rec = md.record_at_key_block(&keyblock).expect("get record");

        create_dir_all("test_output").expect("create test_output dir");
        let mut out = File::create("test_output/mdd_record").expect("create output file");
        out.write_all(&rec).expect("write record to file");

        println!("wrote {} bytes to test_output/mdd_record", rec.len());
    }

    #[test]
    fn write_all_keys_to_keys_txt() {
        let f = File::open(SAMPLE_PATH).expect("open mdx file");
        let mut md = mdict_tools::Mdict::new(f).expect("open mdx via Mdict");

        let mut out = File::create("test_output/all_keys.txt").expect("create output file");

        for i in 0..md.key_block_index.key_section.num_entries as usize {
            if let Ok(Some(kb)) = md.key_block_index.get(&mut md.reader, i) {
                writeln!(out, "{}", kb.key_text).expect("write key to file");
            }
        }
    }

    #[test]
    fn test_mdx_reindexer() {
        let readings_list = mdx_conversion::reindexing::build_readings_list_from_path(SAMPLE_PATH)
            .expect("build readings list from path");
        println!("Readings list has {} entries", readings_list.len());
        for (link, keys) in readings_list.iter().take(10) {
            println!("Link '{}' has {} keys: {:?}", link, keys.len(), keys);
        }

        // Write the readings list to a compressed file
        mdx_conversion::reindexing::write_compressed_readings_list(
            &readings_list,
            "test_output/readings_list.txt",
        )
        .expect("write readings list");

        // Read it back and verify it matches the original
        let readings_list2 = mdx_conversion::reindexing::read_compressed_readings_list(
            "test_output/readings_list.txt",
        )
        .expect("read readings list");

        assert_eq!(
            readings_list, readings_list2,
            "Readings list should match after write and read"
        );
    }

    #[test]
    fn test_fst_indexing_creation() {
        let f = File::open(SAMPLE_PATH).expect("open mdx file");
        let mut mdict = Mdict::new_with_cache(f, usize::MAX).expect("open mdx via Mdict");
        let readings_list = mdx_conversion::reindexing::read_compressed_readings_list(
            "test_output/readings_list.txt",
        )
        .expect("read readings list");

        mdx_conversion::fst_indexing::create_fst_index(
            &mut mdict,
            &readings_list,
            "test_output/fst_index.fst",
            "test_output/fst_index_values.txt",
            "test_output/record_section.dat",
        )
        .expect("create fst index");
    }

    #[test]
    fn test_fst_searching() {
        let fst_map = FSTMap::load_from_path(
            "test_output/fst_index.fst",
            "test_output/fst_index_values.txt",
            "test_output/record_section.dat",
        )
        .expect("load fst index");

        let start_time = Instant::now();
        let test_key = "辞";
        let links: Vec<(String, u64)> = fst_map.get_link_for_key_dedup(test_key).collect();
        println!("Links for key '{}':", test_key);

        for (key, value) in links {
            let (readings_entry, record_size) = fst_map.get_readings(value).unwrap();
            println!("  {} => {} with record size {:?} and readings {:?}", key, value, record_size, readings_entry.readings);
            // let record = fst_map.get_record(value, &mut File::open("test_output/record_section.dat").expect("open record section file"), record_size).expect("get record");
            // println!("    record size: {} : {}", record.len(), String::from_utf8_lossy(&record));
        }

        let elapsed = start_time.elapsed();

        println!(
            "FST search for key '{}' took {:.6} seconds",
            test_key,
            elapsed.as_secs_f64()
        );
    }

    #[test]
    fn test_search_jisho_in_fst_and_mdict_ensure_same() {
        use std::collections::BTreeSet;

        let test_key = "辞書";

        let fst_map = FSTMap::load_from_path(
            "test_output/fst_index.fst",
            "test_output/fst_index_values.txt",
            "test_output/record_section.dat",
        )
        .expect("load fst index");

        let mut fst_results: Vec<(String, u64)> = Vec::new();
        let mut fst_stream = fst_map.get_link_for_key(test_key);
        while let Some((k, v)) = fst_stream.next() {
            println!("  [fst] key='{}' link={}", String::from_utf8_lossy(k), v);
            fst_results.push((String::from_utf8_lossy(k).into_owned(), v));
        }

        assert!(
            !fst_results.is_empty(),
            "Expected FST results for prefix '{}'",
            test_key
        );

        let f = File::open(SAMPLE_PATH).expect("open mdx file");
        let mut mdict = Mdict::new_with_cache(f, usize::MAX).expect("open mdx via Mdict");
        let mut md_iter = mdict.search_keys_prefix(test_key).expect("search mdict");
        let md_results = md_iter.collect_to_vec().expect("collect mdict results");

        assert!(
            !md_results.is_empty(),
            "Expected MDict results for prefix '{}'",
            test_key
        );

        let fst_key_set: BTreeSet<String> = fst_results.iter().map(|(k, _)| k.clone()).collect();
        let md_key_set: BTreeSet<String> = md_results.iter().map(|kb| kb.key_text.clone()).collect();

        assert_eq!(
            fst_key_set, md_key_set,
            "Prefix key sets should match between FST and MDict for '{}'",
            test_key
        );

        let (_, fst_link) = fst_results
            .iter()
            .find(|(k, _)| k == test_key)
            .expect("find exact key in FST results");
        let (readings_entry, record_size) = fst_map
            .get_readings(*fst_link)
            .expect("get readings entry from fst values");

        let fst_record = fst_map
            .get_record(*fst_link, record_size)
            .expect("get record from fst map");

        let md_key_block = md_results
            .iter()
            .find(|kb| kb.key_text == test_key)
            .expect("find exact key in mdict results")
            .clone();
        let md_record = get_record_for_key_id(&mut mdict, &md_key_block);

        let fst_record_str = String::from_utf8_lossy(&fst_record);
        let md_record_str = String::from_utf8_lossy(&md_record);

        println!(
            "Compared exact key '{}' with link {} and readings {:?}",
            test_key, readings_entry.link_id, readings_entry.readings
        );

        assert_eq!(
            fst_record_str, md_record_str,
            "Record string content should match for exact key '{}'",
            test_key
        );
    }

    #[test]
    fn test_record_section_consistency() {
        // Test that records retrieved from the original MDX file match those from the converted record section
        let f = File::open(SAMPLE_PATH).expect("open mdx file");
        let mut mdict = Mdict::new_with_cache(f, usize::MAX).expect("open mdx via Mdict");

        // Load the FST map and record section
        let fst_map = FSTMap::load_from_path(
            "test_output/fst_index.fst",
            "test_output/fst_index_values.txt",
            "test_output/record_section.dat",
        )
        .expect("load fst index");

        // Get some keys from the original MDX file to test
        let test_keys = vec!["辞", "辞書", "日本語"];

        for key in test_keys {
            if let Some(link) = fst_map.get(key) {
                // Get record size from FST map
                let Some((_, record_size)) = fst_map.get_readings(link) else {
                    println!("No readings entry found for link {} of key '{}'", link, key);
                    continue;
                };

                if record_size.is_none() {
                    println!("Record goes to end of file for link {} of key '{}'", link, key);
                    continue;
                }

                let record_size = record_size.unwrap();

                // Verify that we get a reasonable record size (not zero)
                assert!(
                    record_size > 0,
                    "Record size should be greater than 0 for key '{}'",
                    key
                );
                println!(
                    "Key '{}' has link {} with record size {:?}",
                    key, link, record_size
                );
            } else {
                println!("No link found for key '{}'", key);
            }
        }
    }

    // #[test]
    // fn test_record_content_consistency() {
    //     // Test that the converted record section can be read and contains valid data
    //     use std::io::BufReader;

    //     // First, let's verify we can read the original record section
    //     let f = File::open(SAMPLE_PATH).expect("open mdx file");
    //     let mut mdict = Mdict::new_with_cache(f, usize::MAX).expect("open mdx via Mdict");

    //     // Check that the original record section has data
    //     assert!(
    //         mdict.record_section.record_index_prefix_sum.len() > 1,
    //         "Original record section should have more than one record index"
    //     );
    //     println!(
    //         "Original record section has {} indices",
    //         mdict.record_section.record_index_prefix_sum.len()
    //     );

    //     // Test reading from the converted record section file
    //     let record_file =
    //         File::open("test_output/record_section.dat").expect("open record section file");
    //     let mut reader = BufReader::new(record_file);

    //     let converted_record_section =
    //         mdx_conversion::records::RecordSection::parse(&mut reader, 0)
    //             .expect("parse converted record section");

    //     for (i, (orig_idx, conv_idx)) in mdict
    //         .record_section
    //         .record_index_prefix_sum
    //         .iter()
    //         .zip(converted_record_section.record_index_prefix_sum.iter())
    //         .enumerate()
    //     {
    //         assert_eq!(
    //             orig_idx.compressed_size, conv_idx.compressed_size,
    //             "Compressed size should match for index {}",
    //             i
    //         );
    //         assert_eq!(
    //             orig_idx.uncompressed_size, conv_idx.uncompressed_size,
    //             "Uncompressed size should match for index {}",
    //             i
    //         );
    //     }

    //     println!("Record section conversion test passed!");
    // }

    // #[test]
    // fn test_converted_record_decode_matches_original() {
    //     use std::io::BufReader;

    //     let f = File::open(SAMPLE_PATH).expect("open mdx file");
    //     let mut mdict = Mdict::new_with_cache(f, usize::MAX).expect("open mdx via Mdict");

    //     let fst_map = FSTMap::load_from_path(
    //         "test_output/fst_index.fst",
    //         "test_output/fst_index_values.txt",
    //         "test_output/record_section.dat",
    //     )
    //     .expect("load fst index");

    //     let record_file = File::open("test_output/record_section.dat").expect("open record file");
    //     let mut record_reader = BufReader::new(record_file);
    //     let _converted = mdx_conversion::records::RecordSection::parse(&mut record_reader, 0)
    //         .expect("parse converted record section");

    //     for key in ["辞", "辞書", "日本語"] {
    //         let Some(link) = fst_map.get(key) else {
    //             continue;
    //         };
    //         let Some((_readings_entry, record_size)) = fst_map.get_readings(link) else {
    //             continue;
    //         };

    //         let converted_record = fst_map
    //             .get_record(link, &mut record_reader, record_size)
    //             .expect("get record from converted section");

    //         let mut iter = mdict.search_keys_prefix(key).expect("search key in original mdict");
    //         let key_block = iter
    //             .collect_to_vec()
    //             .expect("collect original key matches")
    //             .into_iter()
    //             .find(|kb| kb.key_text == key)
    //             .expect("find exact original key match");
    //         let original_record = get_record_for_key_id(&mut mdict, &key_block);

    //         assert_eq!(
    //             converted_record, original_record,
    //             "Converted decode should match original for key '{}'",
    //             key
    //         );
    //     }
    // }
}
