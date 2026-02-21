
#[cfg(test)]
mod tests {
    use std::fs::{File, create_dir_all};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::Instant;
    use mdict_tools::types::KeyBlock;
    use sysinfo::{System, get_current_pid, ProcessesToUpdate};

    const SAMPLE_PATH: &str = "resources/jitendex/jitendex.mdx";
    const SAMPLE_MDD_PATH: &str = "resources/jitendex/jitendex.mdd";

    fn get_record_for_key_id(md: &mut mdict_tools::Mdict<File>, key_block: &KeyBlock) -> Vec<u8> {
        let rec = md.record_at_key_block(key_block).unwrap_or_else(|_| Vec::new());
            if let Some(suffix) = rec.strip_prefix(b"@@@LINK=") {
                    if let Ok(tag) = std::str::from_utf8(suffix) {
                        println!("Found link record for key_id {}: tag='{}'", key_block.key_id, tag);
                        let key_index: Vec<_> = match md.search_keys_prefix(tag) {
                            Ok(it) => it.filter_map(|r| r.ok()).collect(),
                            Err(_) => vec![],
                        };
                        
                        if let Some(k) = key_index.first() {
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
        let key_section = mdict_tools::format::KeySection::read_from(&mut file, &header).expect("read key section");

        println!("KeySection: num_blocks = {} num_entries = {}", key_section.num_blocks, key_section.num_entries);
        if !key_section.key_info_blocks.is_empty() {
            let kb0 = &key_section.key_info_blocks[0];
            println!("First key block[0]: num_entries={} first='{}' last='{}' comp={} decomp={}",
                kb0.num_entries, kb0.first, kb0.last, kb0.compressed_size, kb0.decompressed_size);
        }

        let limit = std::cmp::min(8usize, key_section.key_info_blocks.len());
        println!("Printing {} key blocks summaries:", limit);
        for (i, kb) in key_section.key_info_blocks.iter().take(limit).enumerate() {
            println!("block {}: num_entries={} first='{}' last='{}' comp={} decomp={}",
                i, kb.num_entries, kb.first, kb.last, kb.compressed_size, kb.decompressed_size);
        }
    }

    #[test]
    fn decode_first_key_block_and_print() {
        let mut file = File::open(SAMPLE_PATH).expect("open mdx file");
        let header = mdict_tools::format::HeaderInfo::read_from(&mut file).expect("read header");

        let mut file = File::open(SAMPLE_PATH).expect("open mdx file");
        let key_section = mdict_tools::format::KeySection::read_from(&mut file, &header).expect("read key section");

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
            println!("parsed header -> encoding (LE) = {} ; checksum (BE) = {}", enc_le, chk_be);
        }

        match mdict_tools::format::decode_format_block(&buf) {
            Ok(decompressed) => {
                let adler = minilzo_rs::adler32(&decompressed);
                println!("decoded len = {} ; adler32 = {}", decompressed.len(), adler);
                println!("first 128 bytes of decompressed (lossy): {}", String::from_utf8_lossy(&decompressed[..std::cmp::min(128, decompressed.len())]));
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

        let prefix = "辞書";
        println!("[new api] searching for prefix '{}', max 10", prefix);

        let start = Instant::now();
        let iter = md.search_keys_prefix(prefix).expect("search");
        let res: Vec<_> = iter.map(|r| r.expect("iter result")).collect();
        let elapsed = start.elapsed();

        sys.refresh_processes(ProcessesToUpdate::All, true);
        let mem_after = sys.process(pid).map(|p| p.memory()).unwrap_or(0);
        let virt_after = sys.process(pid).map(|p| p.virtual_memory()).unwrap_or(0);

        println!("[new api] found {} keys starting with '{}'", res.len(), prefix);
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
        for kb in res.iter() {
            println!("[new api] key_id={} key='{}'", kb.key_id, kb.key_text);
            let rec_bytes = get_record_for_key_id(&mut md, &kb);
            println!("[new api] record for key_id {}: {}", kb.key_id, String::from_utf8_lossy(&rec_bytes));
        }

        assert!(!res.is_empty(), "expected at least one key for prefix '{}'", prefix);
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
        let key_section = mdict_tools::format::KeySection::read_from(&mut file, &header).expect("read key section");

        println!("KeySection: num_blocks = {} num_entries = {}", key_section.num_blocks, key_section.num_entries);
        if !key_section.key_info_blocks.is_empty() {
            let kb0 = &key_section.key_info_blocks[0];
            println!("First key block[0]: num_entries={} first='{}' last='{}' comp={} decomp={}",
                kb0.num_entries, kb0.first, kb0.last, kb0.compressed_size, kb0.decompressed_size);
        }

        let limit = std::cmp::min(8usize, key_section.key_info_blocks.len());
        println!("Printing {} key blocks summaries:", limit);
        for (i, kb) in key_section.key_info_blocks.iter().take(limit).enumerate() {
            println!("block {}: num_entries={} first='{}' last='{}' comp={} decomp={}",
                i, kb.num_entries, kb.first, kb.last, kb.compressed_size, kb.decompressed_size);
        }
    }

    #[test]
    fn write_mdd_record_to_output() {
        let f = File::open(SAMPLE_MDD_PATH).expect("open mdd file");
        let mut md = mdict_tools::Mdict::new(f).expect("open mdd via Mdict");

        let mut hf = File::open(SAMPLE_MDD_PATH).expect("open mdd file for header");
        let header = mdict_tools::format::HeaderInfo::read_from(&mut hf).expect("read header");
        let mut kf = File::open(SAMPLE_MDD_PATH).expect("open mdd file for keys");
        let key_section = mdict_tools::format::KeySection::read_from(&mut kf, &header).expect("read key section");

        if key_section.key_info_blocks.is_empty() {
            panic!("no key blocks found in MDD");
        }

        let first_key = key_section.key_info_blocks[1].first.clone();
        let matches: Vec<_> = md.search_keys_prefix(&first_key).expect("search mdd").map(|r| r.expect("iter result")).collect();
        if matches.is_empty() {
            panic!("no matching key for first key text");
        }

        println!("Found matching key_id {} for first key text '{}'", matches[0].key_id, first_key);

        let keyblock = matches[0].clone();

        let rec = md.record_at_key_block(&keyblock).expect("get record");

        create_dir_all("test_output").expect("create test_output dir");
        let mut out = File::create("test_output/mdd_record").expect("create output file");
        out.write_all(&rec).expect("write record to file");

        println!("wrote {} bytes to test_output/mdd_record", rec.len());
    }
        
}