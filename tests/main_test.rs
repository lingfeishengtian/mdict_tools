
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
}