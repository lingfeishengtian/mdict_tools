use std::fs::create_dir_all;
use std::path::PathBuf;

use mdict_tools::types::{KeyBlock, PrefixSearchCursor};

const SAMPLE_PATH: &str = "resources/jitendex/jitendex.mdx";
const SAMPLE_MDD_PATH: &str = "resources/jitendex/jitendex.mdd";

fn test_output_dir() -> PathBuf {
    std::env::var("TEST_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("test_output"))
}

fn create_or_load_optimized(
    bundle: &mdict_tools::MdictBundle,
    tag: &str,
) -> mdict_tools::MdictOptimized {
    let out_dir = test_output_dir().join("uniffi_optimized");
    create_dir_all(&out_dir).expect("create test output directory");

    let fst_path = out_dir.join(format!("{}.fst", tag));
    let readings_path = out_dir.join(format!("{}_readings.dat", tag));
    let record_path = out_dir.join(format!("{}_records.dat", tag));

    if fst_path.exists() && readings_path.exists() && record_path.exists() {
        mdict_tools::mdict_optimized::create_mdict_optimized_from_fst(
            fst_path.to_string_lossy().to_string(),
            readings_path.to_string_lossy().to_string(),
            record_path.to_string_lossy().to_string(),
        )
        .expect("load optimized bundle from existing files")
    } else {
        mdict_tools::mdict_optimized::create_mdict_optimized_from_bundle(
            bundle,
            fst_path.to_string_lossy().to_string(),
            readings_path.to_string_lossy().to_string(),
            record_path.to_string_lossy().to_string(),
        )
        .expect("create optimized bundle")
    }
}

fn legacy_bundle_top_keys(bundle: &mdict_tools::MdictBundle, prefix: &str, limit: usize) -> Vec<KeyBlock> {
    bundle
        .set_search_prefix(prefix)
        .expect("set legacy prefix search");

    let available = bundle.len() as usize;
    let take_n = std::cmp::min(limit, available);
    let mut keys = Vec::with_capacity(take_n);
    for i in 0..take_n {
        let key_block = bundle
            .prefix_search_result_get(i as u64)
            .expect("get legacy prefix search result")
            .expect("legacy prefix result exists");
        keys.push(key_block);
    }
    keys
}

fn optimized_top_keys(
    optimized: &mdict_tools::MdictOptimized,
    prefix: &str,
    limit: usize,
    page_size: u64,
) -> Vec<KeyBlock> {
    let mut collected = Vec::new();
    let mut page = optimized
        .set_search_prefix_paged(prefix, page_size)
        .expect("set optimized paged prefix search");

    collected.extend(page.results.into_iter());
    while collected.len() < limit {
        let Some(cursor) = page.next_cursor.take() else {
            break;
        };
        page = optimized
            .prefix_search_next_page(PrefixSearchCursor {
                after_key: cursor.after_key,
            })
            .expect("fetch next optimized page");
        collected.extend(page.results.into_iter());
    }

    collected.truncate(limit);
    collected
}

fn legacy_bundle_resolved_record(bundle: &mdict_tools::MdictBundle, key_block: &KeyBlock) -> Vec<u8> {
    let mut current = key_block.clone();
    let mut depth = 0usize;

    loop {
        depth += 1;
        if depth > 32 {
            return Vec::new();
        }

        let rec = bundle.record_at(current.clone()).expect("get legacy record");
        let Some(suffix) = rec.strip_prefix(b"@@@LINK=") else {
            return rec;
        };

        let Ok(tag) = std::str::from_utf8(suffix) else {
            return Vec::new();
        };

        bundle
            .set_search_prefix(tag)
            .expect("set prefix while resolving legacy link");
        let Some(next_key) = bundle
            .prefix_search_result_get(0)
            .expect("get first link target")
        else {
            return Vec::new();
        };
        current = next_key;
    }
}

#[test]
fn optimized_keys_match_legacy_bundle() {
    let prefix = "辞書";
    let limit = 20usize;

    let bundle = mdict_tools::mdict_file::create_mdict_bundle(SAMPLE_PATH.into(), SAMPLE_MDD_PATH.into())
        .expect("open legacy bundle");
    let optimized = create_or_load_optimized(&bundle, "optimized_keys_match_legacy_bundle");

    let legacy_keys = legacy_bundle_top_keys(&bundle, prefix, limit);
    let optimized_keys = optimized_top_keys(&optimized, prefix, limit, 8);

    assert!(
        !legacy_keys.is_empty(),
        "expected at least one key for prefix '{}'",
        prefix
    );
    assert_eq!(legacy_keys.len(), optimized_keys.len(), "result length mismatch");

    for (i, (lk, ok)) in legacy_keys.iter().zip(optimized_keys.iter()).enumerate() {
        println!("[compare] Legacy key: id={}, text='{}'", lk.key_id, lk.key_text);
        println!("[compare] Optimized key: id={}, text='{}'", ok.key_id, ok.key_text);
        assert_eq!(&lk.key_text, &ok.key_text, "key_text mismatch at {}", i);
    }
    println!("[compare] Optimized keys match legacy bundle for prefix '辞書'");
}

#[test]
fn optimized_records_match_legacy_bundle_resolved() {
    let prefix = "辞書";
    let limit = 10usize;

    let bundle = mdict_tools::mdict_file::create_mdict_bundle(SAMPLE_PATH.into(), SAMPLE_MDD_PATH.into())
        .expect("open legacy bundle");
    let optimized = create_or_load_optimized(&bundle, "optimized_records_match_legacy_bundle_resolved");

    let legacy_keys = legacy_bundle_top_keys(&bundle, prefix, limit);
    let optimized_keys = optimized_top_keys(&optimized, prefix, limit, 8);
    assert_eq!(legacy_keys.len(), optimized_keys.len(), "result length mismatch");

    for (i, (legacy_key, optimized_key)) in legacy_keys.iter().zip(optimized_keys.iter()).enumerate() {
        assert_eq!(
            legacy_key.key_text, optimized_key.key_text,
            "key_text mismatch at {}",
            i
        );
        let legacy_record = legacy_bundle_resolved_record(&bundle, legacy_key);
        let optimized_record = optimized
            .record_at(optimized_key.clone())
            .expect("get optimized record");
        assert_eq!(
            legacy_record, optimized_record,
            "resolved record bytes mismatch at {}",
            i
        );
    }
    println!("[compare] Optimized records match resolved legacy bundle records for prefix '辞書'");
}
