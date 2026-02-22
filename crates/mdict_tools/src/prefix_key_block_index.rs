use std::io::{Read, Seek};

use crate::Mdict;
use crate::error::Result;
use crate::random_access_key_blocks::KeyBlockIndex;
use crate::types::KeyBlock;

pub struct PrefixKeyBlockIndex<'a, R: Read + Seek> {
    mdict: &'a mut Mdict<R>,
    prefix: String,
    start_index: usize,
    end_index: usize,
    cursor: usize,
}

impl<'a, R: Read + Seek> PrefixKeyBlockIndex<'a, R> {
    pub fn new(mdict: &'a mut Mdict<R>, prefix: &str) -> Result<Self> {
        let (start, end) = mdict.key_block_index.prefix_range_bounds(&mut mdict.reader, prefix)?.ok_or_else(|| {
            crate::error::MDictError::InvalidArgument("Prefix not found".to_string())
        })?;

        println!("Prefix '{}' matches key blocks in range [{}, {})", prefix, start, end);

        Ok(Self {
            mdict,
            prefix: prefix.to_string(),
            start_index: start,
            end_index: end,
            cursor: 0,
        })
    }
    
    pub fn len(&self) -> usize {
        self.end_index - self.start_index
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&mut self, index: usize) -> Result<Option<KeyBlock>> {
        if self.start_index + index >= self.end_index {
            Ok(None)
        } else {
            let mdict_mut = &mut self.mdict;
            mdict_mut.key_block_index.get(&mut mdict_mut.reader, self.start_index + index)
        }
    }

    pub fn next(&mut self) -> Result<Option<KeyBlock>> {
        if self.start_index + self.cursor >= self.end_index {
            Ok(None)
        } else {
            let mdict_mut = &mut self.mdict;
            let result = mdict_mut.key_block_index.get(&mut mdict_mut.reader, self.start_index + self.cursor)?;
            self.cursor += 1;
            Ok(result)
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
        &self.prefix
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = 0;
    }

    pub fn take(&mut self, n: usize) -> Result<Vec<KeyBlock>> {
        let mut result = Vec::new();

        for _ in 0..n {
            if let Some(key_block) = self.next()? {
                result.push(key_block);
            } else {
                break;
            }
        }
        
        Ok(result)
    }
}
