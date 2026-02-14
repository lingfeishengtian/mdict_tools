use std::io::{Read, Seek};

use crate::error::{Result, MDictError};
use crate::types::{KeyBlock, SearchHit};

/// Public `Mdict` API using a generic `Read + Seek` reader.
pub struct Mdict<R: Read + Seek> {
    reader: R,
}

impl<R: Read + Seek> Mdict<R> {
    /// Create from an arbitrary reader implementing `Read + Seek`.
    pub fn new(reader: R) -> Self {
        Mdict { reader }
    }

    /// Load header and validate. Not implemented in scaffold.
    pub fn load_header(&mut self) -> Result<()> {
        Err(MDictError::UnsupportedFeature("header parsing not yet implemented".to_owned()))
    }

    /// Search for the first matching entry and return a `SearchHit`.
    /// Returns `Ok(None)` when no match is found.
    pub fn search_first(&mut self, _query: &str) -> Result<Option<SearchHit>> {
        // TODO: implement search; scaffold returns None for now
        Ok(None)
    }

    /// Read the record for a specific key id. Not implemented in scaffold.
    pub fn record_at(&mut self, _key_id: u64) -> Result<String> {
        Err(MDictError::UnsupportedFeature("record decoding not yet implemented".to_owned()))
    }
}