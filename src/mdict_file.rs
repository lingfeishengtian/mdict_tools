use std::fs::File;

use boltffi::export;

use crate::{
    error::MDictError, prefix_key_block_index::PrefixKeyBlockIndexInternal,
    seekable_mmap::SeekableMmap, types::KeyBlock, Mdict,
};

pub struct MdictBundle {
    mdx: Mdict<SeekableMmap>,
    mdd: Option<Mdict<SeekableMmap>>,

    current_mdx_prefix_key_index: Option<PrefixKeyBlockIndexInternal>,
}

#[export]
impl MdictBundle {
    pub fn new(mdx_path: &str, mdd_path: Option<String>) -> Result<Self, MDictError> {
        let mdx_file = File::open(mdx_path)?;
        let mdx_mmap = SeekableMmap::open(&mdx_file)?;

        let mdd_mmap = if let Some(mdd_path) = mdd_path {
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

        Ok(Self {
            mdx,
            mdd,
            current_mdx_prefix_key_index: None,
        })
    }

    pub fn set_search_prefix(&mut self, prefix: String) -> Result<(), MDictError> {
        let prefix_index = self
            .mdx
            .key_block_index
            .prefix_range_bounds(&mut self.mdx.reader, &prefix)?
            .ok_or_else(|| MDictError::InvalidArgument("Prefix not found".to_string()))?;

        self.current_mdx_prefix_key_index = Some(PrefixKeyBlockIndexInternal::new(
            prefix.to_string(),
            prefix_index.0,
            prefix_index.1,
        ));
        Ok(())
    }

    pub fn prefix_search_result_get(
        &mut self,
        index: usize,
    ) -> Result<Option<KeyBlock>, MDictError> {
        let prefix_index = self
            .current_mdx_prefix_key_index
            .as_ref()
            .ok_or_else(|| MDictError::InvalidArgument("Search prefix not set".to_string()))?;

        let global_index = prefix_index.get_global_index(index).ok_or_else(|| {
            MDictError::InvalidArgument(
                "Index out of bounds for current prefix search results".to_string(),
            )
        })?;

        self.mdx
            .key_block_index
            .get(&mut self.mdx.reader, global_index)
            .map_err(MDictError::from)
    }

    pub fn record_at(&mut self, key_block: KeyBlock) -> Result<Vec<u8>, MDictError> {
        let record_data = self.mdx.record_at_key_block(&key_block)?;
        Ok(record_data)
    }

    pub fn mdd_resource(&mut self, key: String) -> Result<Option<Vec<u8>>, MDictError> {
        if let Some(mdd) = &mut self.mdd {
            let key_block_idx = mdd
                .key_block_index
                .index_for(&mut mdd.reader, &key)?
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
}
