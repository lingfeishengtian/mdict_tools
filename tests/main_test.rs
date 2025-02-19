
#[cfg(test)]
mod tests {
    #[test]
    fn open_file() {
        let file_path = "resources/jitendex/jitendex.mdx";
        let mut mdict = mdict_tools::MDict::open(file_path).unwrap();
        let curr_time = std::time::Instant::now();

        if let Some(mut search_result) = mdict.search_query("食う") {
            // while let Some(record) = search_result.next() {
            //     // println!("Key info: {:?} Record: {}", record.0, record.1);
            // }
            // List 10 entries
            for _ in 0..10 {
                if let Some(record) = search_result.next() {
                    println!("Key info: {:?} Record: {}", record.0, record.1);
                } else {
                    break;
                }
            }
        }
    }
}