
use std::fs::File;
use mdict_tools::types::KeyBlock;

const SAMPLE_PATH: &str = "resources/jitendex/jitendex.mdx";
const SAMPLE_MDD_PATH: &str = "resources/jitendex/jitendex.mdd";

fn get_record_for_key_id(md: &mut mdict_tools::Mdict<File>, key_block: &KeyBlock) -> Vec<u8> {
    let rec = md
        .record_at_key_block(key_block)
        .unwrap_or_else(|_| Vec::new());
    if let Some(suffix) = rec.strip_prefix(b"@@@LINK=") {
        if let Ok(tag) = std::str::from_utf8(suffix) {
            let key_index: Vec<_> = match md.search_keys_prefix(tag) {
                Ok(mut it) => it.collect_to_vec().unwrap_or_else(|_| vec![]),
                Err(_) => vec![],
            };
            if let Some(k) = key_index.first() {
                return get_record_for_key_id(md, k);
            }
        }
        return Vec::new();
    }
    rec
}

#[test]
fn compare_search_for_jisho_keys() {
    // Direct API
    let mut md = mdict_tools::Mdict::new(File::open(SAMPLE_PATH).expect("open mdx")).expect("open mdx via Mdict");
    let prefix = "辞書";
    let mut iter = md.search_keys_prefix(prefix).expect("search");
    let direct_keys: Vec<_> = iter.take(10).unwrap_or(Vec::new());

    // UniFFI API
    let bundle = mdict_tools::mdict_file::create_mdict_bundle(SAMPLE_PATH.into(), SAMPLE_MDD_PATH.into()).expect("open bundle");
    bundle.set_search_prefix(prefix).expect("set prefix");
    let len = bundle.len() as usize;
    assert!(len > 0, "expected at least one key for prefix '辞書'");
    let mut uniffi_keys = Vec::new();
    for i in 0..std::cmp::min(10, len) {
        let kb = bundle
            .prefix_search_result_get(i as u64)
            .expect("get keyblock")
            .expect("keyblock exists");
        uniffi_keys.push((kb.key_id, kb.key_text.clone()));
    }

    // Compare keys
    for (i, (dk, uk)) in direct_keys.iter().zip(uniffi_keys.iter()).enumerate() {
        println!("[compare] Direct key: id={}, text='{}'", dk.key_id, dk.key_text);
        println!("[compare] UniFFI key: id={}, text='{}'", uk.0, uk.1);
        assert_eq!(dk.key_id, uk.0, "key_id mismatch at {}", i);
        assert_eq!(&dk.key_text, &uk.1, "key_text mismatch at {}", i);
    }
    println!("[compare] All keys match for prefix '辞書' (first 10 results)");
}

#[test]
fn compare_search_for_jisho_records() {
    // Direct API
    let mut md = mdict_tools::Mdict::new(File::open(SAMPLE_PATH).expect("open mdx")).expect("open mdx via Mdict");
    let prefix = "辞書";
    let mut iter = md.search_keys_prefix(prefix).expect("search");
    let direct_keys: Vec<_> = iter.take(10).unwrap_or(Vec::new());
    let direct_records: Vec<_> = direct_keys.iter().map(|kb| get_record_for_key_id(&mut md, kb)).collect();

    // UniFFI API
    let bundle = mdict_tools::mdict_file::create_mdict_bundle(SAMPLE_PATH.into(), SAMPLE_MDD_PATH.into()).expect("open bundle");
    bundle.set_search_prefix(prefix).expect("set prefix");
    let len = bundle.len() as usize;
    assert!(len > 0, "expected at least one key for prefix '辞書'");
    let mut uniffi_records = Vec::new();
    for i in 0..std::cmp::min(10, len) {
        let kb = bundle
            .prefix_search_result_get(i as u64)
            .expect("get keyblock")
            .expect("keyblock exists");
        let rec_bytes = bundle.record_at(kb.clone()).expect("get record");
        uniffi_records.push(rec_bytes);
    }

    // Compare records
    for (i, (dr, ur)) in direct_records.iter().zip(uniffi_records.iter()).enumerate() {
        assert_eq!(dr, ur, "record bytes mismatch at {}", i);
    }
    println!("[compare] All records match for prefix '辞書' (first 10 results)");
}
