
#[cfg(test)]
mod tests {
    #[test]
    fn open_file() {
        let file_path = "resources/jitendex/jitendex.mdx";
        let mdict = mdict_tools::MDict::open(file_path).unwrap();
    }
}