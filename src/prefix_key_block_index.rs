use std::io::{Read, Seek};

use boltffi::{data, export};

use crate::Mdict;
use crate::error::Result;
use crate::types::KeyBlock;

/// Internal, non-borrowing prefix view. Holds only index bounds and cursor
/// so it can live without borrowing the containing `Mdict`.
pub struct PrefixKeyBlockIndexInternal {
    pub prefix: String,
    pub start_index: usize,
    pub end_index: usize,
    pub cursor: usize,
}

#[export]
impl PrefixKeyBlockIndexInternal {
    pub fn new(prefix: String, start_index: usize, end_index: usize) -> Self {
        Self { prefix, start_index, end_index, cursor: 0 }
    }

    pub fn len(&self) -> usize {
        self.end_index.saturating_sub(self.start_index)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    pub fn get_global_index(&self, idx: usize) -> Option<usize> {
        let g = self.start_index.checked_add(idx)?;
        if g < self.end_index { Some(g) } else { None }
    }

    pub fn next_global_index(&mut self) -> Option<usize> {
        let g = self.start_index.checked_add(self.cursor)?;
        if g < self.end_index {
            self.cursor = self.cursor.saturating_add(1);
            Some(g)
        } else {
            None
        }
    }

    pub fn take_indices(&mut self, n: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for _ in 0..n {
            if let Some(g) = self.next_global_index() {
                out.push(g);
            } else {
                break;
            }
        }
        out
    }
}

pub struct PrefixKeyBlockIndex<'a, R: Read + Seek> {
    mdict: &'a mut Mdict<R>,
    inner: PrefixKeyBlockIndexInternal,
}

impl<'a, R: Read + Seek> PrefixKeyBlockIndex<'a, R> {
    pub fn new(mdict: &'a mut Mdict<R>, prefix: &str) -> Result<Self> {
        let (start, end) = mdict.key_block_index.prefix_range_bounds(&mut mdict.reader, prefix)?.ok_or_else(|| {
            crate::error::MDictError::InvalidArgument("Prefix not found".to_string())
        })?;

        println!("Prefix '{}' matches key blocks in range [{}, {})", prefix, start, end);

        Ok(Self {
            mdict,
            inner: PrefixKeyBlockIndexInternal::new(prefix.to_string(), start, end),
        })
    }
    
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn get(&mut self, index: usize) -> Result<Option<KeyBlock>> {
        let g = match self.inner.get_global_index(index) {
            Some(v) => v,
            None => return Ok(None),
        };
        let mdict_mut = &mut self.mdict;
        mdict_mut.key_block_index.get(&mut mdict_mut.reader, g)
    }

    pub fn next(&mut self) -> Result<Option<KeyBlock>> {
        match self.inner.next_global_index() {
            Some(g) => {
                let mdict_mut = &mut self.mdict;
                let result = mdict_mut.key_block_index.get(&mut mdict_mut.reader, g)?;
                Ok(result)
            }
            None => Ok(None),
        }
    }

    pub fn collect_to_vec(&mut self) -> Result<Vec<KeyBlock>> {
        let mut result = Vec::new();
        while let Some(key_block) = self.next()? {
            result.push(key_block);
        }
        Ok(result)
    }

    pub fn prefix(&self) -> &str {
        self.inner.prefix()
    }

    pub fn reset_cursor(&mut self) {
        self.inner.reset();
    }

    pub fn take(&mut self, n: usize) -> Result<Vec<KeyBlock>> {
        let mut result = Vec::new();

        for idx in self.inner.take_indices(n) {
            let mdict_mut = &mut self.mdict;
            if let Some(kb) = mdict_mut.key_block_index.get(&mut mdict_mut.reader, idx)? {
                result.push(kb);
            }
        }

        Ok(result)
    }
}
