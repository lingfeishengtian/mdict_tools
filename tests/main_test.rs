
#[cfg(test)]
mod tests {
    #[test]
    fn open_file() {
        let file_path = "resources/jitendex/jitendex.mdx";
        let mut mdict = mdict_tools::MDict::open(file_path).unwrap();
        let curr_time = std::time::Instant::now();
        // println!("{:?}", &mut mdict.search_query("源"));
        println!("{:?}", curr_time.elapsed());
    }
}