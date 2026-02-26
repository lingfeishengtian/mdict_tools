use std::{
    fs::File,
    io::{Read, Seek},
    iter::Map,
    sync::{Arc, Mutex},
};

use crate::{
    error::MDictError, prefix_key_block_index::PrefixKeyBlockIndexInternal,
    seekable_mmap::SeekableMmap, types::KeyBlock, Mdict,
};

#[derive(uniffi::Object)]
pub struct MdictBundle {
    mdx: Mutex<Mdict<SeekableMmap>>,
    mdd: Mutex<Option<Mdict<SeekableMmap>>>,

    current_mdx_prefix_key_index: Mutex<Option<PrefixKeyBlockIndexInternal>>,
}

#[uniffi::export]
pub fn create_mdict_bundle(mdx_path: String, mdd_path: String) -> Result<MdictBundle, MDictError> {
    let mdx_file = File::open(mdx_path)?;
    let mdx_mmap = SeekableMmap::open(&mdx_file)?;

    let mdd_mmap = if !mdd_path.is_empty() {
        let mdd_file = File::open(mdd_path)?;
        Some(SeekableMmap::open(&mdd_file)?)
    } else {
        None
    };

    let mdx = Mdict::new(mdx_mmap)?;
    let mdd = if let Some(mdd_mmap) = mdd_mmap {
        Some(Mdict::new(mdd_mmap)?)
    } else {
        None
    };

    Ok(MdictBundle {
        mdx: Mutex::new(mdx),
        mdd: Mutex::new(mdd),
        current_mdx_prefix_key_index: Mutex::new(None),
    })
}

impl<R: Read + Seek> Mdict<R> {
    pub fn prefix_range_bounds(
        &mut self,
        prefix: &str,
    ) -> Result<Option<(usize, usize)>, MDictError> {
        self.key_block_index
            .prefix_range_bounds(&mut self.reader, prefix)
    }

    pub fn get(&mut self, index: usize) -> Result<Option<KeyBlock>, MDictError> {
        self.key_block_index.get(&mut self.reader, index)
    }
}

#[uniffi::export]
impl MdictBundle {
    pub fn set_search_prefix(&self, prefix: &str) -> Result<(), MDictError> {
        let mut mdx = self.mdx.lock().unwrap();

        let prefix_index = mdx.prefix_range_bounds(prefix)?.ok_or_else(|| {
            MDictError::InvalidArgument(format!("Prefix '{}' not found in MDX", prefix))
        })?;

        *self.current_mdx_prefix_key_index.lock().unwrap() = Some(
            PrefixKeyBlockIndexInternal::new(prefix.to_string(), prefix_index.0, prefix_index.1),
        );
        Ok(())
    }

    pub fn prefix_search_result_get(&self, index: u64) -> Result<Option<KeyBlock>, MDictError> {
        let prefix_index_guard = self.current_mdx_prefix_key_index.lock().unwrap();
        let prefix_index = prefix_index_guard
            .as_ref()
            .ok_or_else(|| MDictError::InvalidArgument("Search prefix not set".to_string()))?;

        let global_index = prefix_index
            .get_global_index(index as usize)
            .ok_or_else(|| {
                MDictError::InvalidArgument(
                    "Index out of bounds for current prefix search results".to_string(),
                )
            })?;
        drop(prefix_index_guard);

        let mut mdx = self.mdx.lock().unwrap();
        mdx.get(global_index).map_err(MDictError::from)
    }

    pub fn record_at(&self, key_block: KeyBlock) -> Result<Vec<u8>, MDictError> {
        let mut mdx = self.mdx.lock().unwrap();
        let record_data = mdx.record_at_key_block(&key_block)?;
        Ok(record_data)
    }

    pub fn mdd_resource(&self, key: &str) -> Result<Option<Vec<u8>>, MDictError> {
        let mut mdd_guard = self.mdd.lock().unwrap();
        if let Some(mdd) = mdd_guard.as_mut() {
            let key_block_idx = mdd
                .key_block_index
                .index_for(&mut mdd.reader, key)?
                .ok_or_else(|| {
                    MDictError::KeyNotFound(format!("Key '{}' not found in MDD", key))
                })?;

            let key_block = mdd
                .key_block_index
                .get(&mut mdd.reader, key_block_idx)?
                .ok_or_else(|| {
                    MDictError::KeyNotFound(format!("Key block for '{}' not found in MDD", key))
                })?;

            let record_data = mdd.record_at_key_block(&key_block)?;
            Ok(Some(record_data))
        } else {
            Ok(None)
        }
    }

    pub fn len(&self) -> u64 {
        self.current_mdx_prefix_key_index
            .lock()
            .unwrap()
            .as_ref()
            .map(|idx| idx.len() as u64)
            .unwrap_or(0) as u64
    }
}
