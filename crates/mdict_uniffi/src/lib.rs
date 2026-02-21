//! UniFFI proc-macro based wrapper around `mdict_tools`.
//!
//! This file uses `uniffi_macros::export` to mark Rust functions that should
//! be exposed to foreign languages. The proc-macro emits metadata (embedded
//! in the compiled library) which UniFFI tooling can later extract to generate
//! language bindings — no `.udl` file required.
uniffi::setup_scaffolding!();

use mdict_tools::Mdict;
use std::path::Path;

use std::sync::{Arc, Mutex};

/// Opaque handle to an open mdict instance. This is exported as an object via
/// UniFFI proc-macros; consumers in Swift will hold a reference to this handle.
#[derive(uniffi_macros::Object)]
pub struct MdictHandle {
    inner: Arc<Mutex<Mdict<std::fs::File>>>,
}

#[uniffi_macros::export]
impl MdictHandle {
    /// Note: construction is provided by the free function `mdict_open`.

    /// Return the dictionary engine version string (from header) if available.
    fn engine_version(&self) -> Option<String> {
        let md = self.inner.lock().ok()?;
        md.header.get("GeneratedByEngineVersion").cloned()
    }

    /// Return the first key in the first key-info block, if present.
    fn first_key(&self) -> Option<String> {
        let md = self.inner.lock().ok()?;
        md.key_section
            .key_info_blocks
            .get(0)
            .map(|kb| kb.first.clone())
    }

    /// Search for keys starting with `prefix`. Returns up to `limit` results.
    fn search_prefix(&self, prefix: String, limit: u32) -> Result<Vec<String>, String> {
        let mut md = self.inner.lock().map_err(|_| "lock failed".to_string())?;
        let it_res = md.search_keys_prefix(&prefix).map_err(|e| format!("search failed: {:?}", e))?;
        let mut out = Vec::new();
        for (i, kb_res) in it_res.enumerate() {
            if (i as u32) >= limit {
                break;
            }
            match kb_res {
                Ok(kb) => out.push(kb.key_text),
                Err(e) => return Err(format!("iterator error: {:?}", e)),
            }
        }
        Ok(out)
    }

    /// Retrieve the raw record bytes for the given `key` (if present).
    fn record_for_key(&self, key: String) -> Result<Option<Vec<u8>>, String> {
        let mut md = self.inner.lock().map_err(|_| "lock failed".to_string())?;

        // Find the first matching key block for the exact key.
        let mut it = match md.search_keys_prefix(&key) {
            Ok(it) => it,
            Err(e) => return Err(format!("search iterator failed: {:?}", e)),
        };

        let next = it.next();
        let kb = match next {
            Some(Ok(kb)) => kb,
            Some(Err(e)) => return Err(format!("iterator error: {:?}", e)),
            None => return Ok(None),
        };

        // Drop the iterator before calling back into `md` mutably.
        drop(it);

        match md.record_at_key_block(&kb) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) => Err(format!("record read failed: {:?}", e)),
        }
    }
}


/// Return the crate version. Small example export to verify the proc-macro
/// pipeline works.
#[uniffi_macros::export]
fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Free function constructor exposed to UniFFI: open the dictionary file and
/// return a managed `MdictHandle`.
#[uniffi_macros::export]
fn mdict_open(path: String) -> Result<MdictHandle, String> {
    let p = Path::new(&path);
    match Mdict::<std::fs::File>::open(p) {
        Ok(m) => Ok(MdictHandle {
            inner: Arc::new(Mutex::new(m)),
        }),
        Err(e) => Err(format!("open failed: {:?}", e)),
    }
}

/// Open the dictionary at `path` and return the first "first" key string from
/// the `KeySection` (if present). This demonstrates exporting a function
/// that wraps the existing `mdict_tools` API.
#[uniffi_macros::export]
fn open_and_first_key(path: String) -> Option<String> {
    // Explicitly specify the `File` instantiation to help type inference in
    // macro-generated contexts.
    match Mdict::<std::fs::File>::open(Path::new(&path)) {
        Ok(mdict) => mdict.key_section.key_info_blocks.get(0).map(|kb| kb.first.clone()),
        Err(_) => None,
    }
}

