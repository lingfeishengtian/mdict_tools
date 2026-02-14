
#[cfg(test)]
mod tests {
    use std::fs::File;

    use mdict_tools::file_reader::FileHandler;

    #[test]
    fn header_parsers_match() {
        let file_path = "resources/jitendex/jitendex.mdx";

        // Old parser (legacy) using FileHandler
        let mut fh = FileHandler::open(file_path).expect("open file for old parser");
        let old = mdict_tools::header::parser::HeaderInfo::retrieve_header(&mut fh)
            .expect("old header parse");

        println!("Old header dict_info: {:?}", old.dict_info());
        println!("Old adler32: {}", old.adler32_checksum());

        // New parser (fresh implementation using binrw + xmlparser)
        let mut file = File::open(file_path).expect("open file for new parser");
        let new = mdict_tools::format::HeaderInfo::read_from(&mut file).expect("new header parse");

        // Compare values and print
        for (k, v) in old.dict_info().iter() {
            let new_v = new.get(k);
            println!("Key: {}  old: {}  new: {:?}", k, v, new_v);
            assert!(new_v.is_some(), "missing key '{}' in new header", k);
            assert_eq!(v, new_v.unwrap());
        }

        println!("New header dict_info_size: {}", new.dict_info_size);
        println!("New adler32: {}", new.adler32_checksum);
    }
}