use crate::error::Result;
use crate::random_access_key_blocks::KeyBlockIndex;
use crate::types::KeyBlock;

/// A view over keys that share a common prefix. This wraps a
/// `KeyBlockIndex` and provides a simple cursor and a few `Vec`-like
/// convenience methods (`len`, `get`, `next`, `collect_to_vec`, etc.).
pub struct PrefixKeyBlockIndex<'a> {
    kbi: &'a mut KeyBlockIndex<'a>,
    prefix: String,

    /// Inclusive start global index for entries matching the prefix.
    start_index: Option<usize>,

    /// Exclusive end global index (cached once computed).
    end_index: Option<usize>,

    /// Cursor (relative to start_index) used by `next()`.
    cursor: usize,
}

impl<'a> PrefixKeyBlockIndex<'a> {
    pub fn new(kbi: &'a mut KeyBlockIndex<'a>, prefix: &str) -> Result<Self> {
    }
}
