use crate::error::Result;
use crate::types::KeyBlock;
use crate::format::{HeaderInfo, KeySection};

use crate::search::iter::{iterator_from_key, ReadSeek, KeyBlocksIterator};

/// Iterator that yields only keys that start with a given prefix.
/// This wraps `KeyBlocksIterator` and stops iteration as soon as a key is
/// encountered that does not start with the provided prefix.
pub struct PrefixKeyIterator<'a> {
    pub inner: KeyBlocksIterator<'a>,
    pub prefix: String,
    pub finished: bool,
}

impl<'a> PrefixKeyIterator<'a> {
    pub fn new(
        reader: &'a mut dyn ReadSeek,
        header: &'a HeaderInfo,
        key_section: &'a KeySection,
        prefix: &str,
    ) -> Result<Self> {
        let inner = iterator_from_key(reader, header, key_section, prefix)?;
        
        Ok(PrefixKeyIterator {
            inner,
            prefix: prefix.to_string(),
            finished: false,
        })
    }
}

impl<'a> Iterator for PrefixKeyIterator<'a> {
    type Item = Result<KeyBlock>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        loop {
            match self.inner.next() {
                Some(Ok(kb)) => {
                    if kb.key_text.starts_with(&self.prefix) {
                        return Some(Ok(kb));
                    } else {
                        if kb.key_text.as_str() < self.prefix.as_str() {
                            continue;
                        } else {
                            self.finished = true;
                            return None;
                        }
                    }
                }
                Some(Err(e)) => return Some(Err(e)),
                None => return None,
            }
        }
    }
}

/// Convenience: create a prefix iterator starting from the block likely to contain `prefix`.
pub fn iterator_from_prefix<'a>(
    reader: &'a mut dyn ReadSeek,
    header: &'a HeaderInfo,
    key_section: &'a KeySection,
    prefix: &str,
) -> Result<PrefixKeyIterator<'a>> {
    PrefixKeyIterator::new(reader, header, key_section, prefix)
}
