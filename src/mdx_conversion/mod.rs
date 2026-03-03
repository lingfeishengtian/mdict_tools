pub mod fst_indexing;
pub mod records;
pub mod reindexing;
pub mod fst_map;
pub mod readings;

const FST_KEY_METADATA_SEPARATOR: &str = "\u{0000}#";

pub(crate) fn with_fst_key_metadata(key: &str, metadata: u64) -> String {
	let mut out = String::with_capacity(key.len() + 2 + 20);
	out.push_str(key);
	out.push_str(FST_KEY_METADATA_SEPARATOR);
	out.push_str(&metadata.to_string());
	out
}

pub(crate) fn strip_fst_key_metadata(key: &str) -> &str {
	if let Some((head, tail)) = key.rsplit_once(FST_KEY_METADATA_SEPARATOR) {
		if tail.parse::<u64>().is_ok() {
			return head;
		}
	}

	key
}