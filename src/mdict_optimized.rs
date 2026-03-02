use std::path::Path;
use std::sync::Mutex;

use crate::error::MDictError;
use crate::mdict_file::MdictBundle;
use crate::mdx_conversion::fst_map::FSTMap;
use crate::types::{BuildProgressStage, KeyBlock, PrefixSearchCursor, PrefixSearchPage};

#[uniffi::export(callback_interface)]
pub trait BuildProgressCallback: Send + Sync {
    fn on_progress(&self, stage: BuildProgressStage, completed: u64, total: u64);
}

#[derive(uniffi::Object)]
pub struct MdictOptimized {
    fst_map: Mutex<FSTMap>,
    current_prefix: Mutex<Option<String>>,
    current_page_size: Mutex<usize>,
}

impl MdictOptimized {
    fn from_fst_files(
        fst_path: impl AsRef<Path>,
        readings_path: impl AsRef<Path>,
        record_path: impl AsRef<Path>,
    ) -> Result<Self, MDictError> {
        let fst_map = FSTMap::load_from_path(fst_path, readings_path, record_path)?;
        Ok(Self {
            fst_map: Mutex::new(fst_map),
            current_prefix: Mutex::new(None),
            current_page_size: Mutex::new(0),
        })
    }

    fn build_page_from_cursor(
        &self,
        cursor_after_key: Option<&str>,
    ) -> Result<PrefixSearchPage, MDictError> {
        let prefix = self
            .current_prefix
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MDictError::InvalidArgument("Search prefix not set".to_string()))?;

        let page_size = *self.current_page_size.lock().unwrap();
        if page_size == 0 {
            return Err(MDictError::InvalidArgument(
                "page_size must be greater than 0".to_string(),
            ));
        }

        let fst_map = self.fst_map.lock().unwrap();
        let (rows, next_key) =
            fst_map.get_link_page_for_prefix(&prefix, cursor_after_key, page_size)?;
        let results = rows
            .into_iter()
            .map(|(key_text, key_id)| KeyBlock { key_id, key_text })
            .collect::<Vec<_>>();

        let next_cursor = next_key.map(|after_key| PrefixSearchCursor { after_key });

        Ok(PrefixSearchPage {
            results,
            next_cursor,
            total_results: None,
        })
    }
}

#[uniffi::export]
pub fn create_mdict_optimized_from_fst(
    fst_path: String,
    readings_path: String,
    record_path: String,
) -> Result<MdictOptimized, MDictError> {
    MdictOptimized::from_fst_files(fst_path, readings_path, record_path)
}

#[uniffi::export]
pub fn create_mdict_optimized_from_bundle(
    bundle: &MdictBundle,
    fst_path: String,
    readings_path: String,
    record_path: String,
) -> Result<MdictOptimized, MDictError> {
    create_mdict_optimized_from_bundle_with_progress(
        bundle,
        fst_path,
        readings_path,
        record_path,
        None,
    )
}

#[uniffi::export]
pub fn create_mdict_optimized_from_bundle_with_progress(
    bundle: &MdictBundle,
    fst_path: String,
    readings_path: String,
    record_path: String,
    progress_callback: Option<Box<dyn BuildProgressCallback>>,
) -> Result<MdictOptimized, MDictError> {
    if let Some(callback) = progress_callback.as_ref() {
        callback.on_progress(BuildProgressStage::Start, 0, 3);
    }

    bundle.build_fst_files_with_progress(
        &fst_path,
        &readings_path,
        &record_path,
        |stage, completed, total| {
            if let Some(callback) = progress_callback.as_ref() {
                callback.on_progress(stage, completed, total);
            }
        },
    )?;

    MdictOptimized::from_fst_files(fst_path, readings_path, record_path)
}

#[uniffi::export]
impl MdictOptimized {
    pub fn set_search_prefix_paged(
        &self,
        prefix: &str,
        page_size: u64,
    ) -> Result<PrefixSearchPage, MDictError> {
        let page_size = usize::try_from(page_size)
            .map_err(|_| MDictError::InvalidArgument("page_size overflow".to_string()))?;
        if page_size == 0 {
            return Err(MDictError::InvalidArgument(
                "page_size must be greater than 0".to_string(),
            ));
        }

        let fst_map = self.fst_map.lock().unwrap();
        drop(fst_map);

        *self.current_page_size.lock().unwrap() = page_size;
        *self.current_prefix.lock().unwrap() = Some(prefix.to_string());
        self.build_page_from_cursor(None)
    }

    pub fn prefix_search_next_page(
        &self,
        cursor: PrefixSearchCursor,
    ) -> Result<PrefixSearchPage, MDictError> {
        if cursor.after_key.is_empty() {
            return Err(MDictError::InvalidArgument(
                "cursor.after_key must not be empty".to_string(),
            ));
        }
        self.build_page_from_cursor(Some(&cursor.after_key))
    }

    pub fn record_at(&self, key_block: KeyBlock) -> Result<Vec<u8>, MDictError> {
        let fst_map = self.fst_map.lock().unwrap();
        let (_, record_size) = fst_map.get_readings_result(key_block.key_id)?;
        fst_map.get_record_result(key_block.key_id, record_size)
    }

    pub fn get_readings(&self, key_block: KeyBlock) -> Result<Vec<String>, MDictError> {
        let fst_map = self.fst_map.lock().unwrap();
        let (readings_entry, _) = fst_map.get_readings_result(key_block.key_id)?;
        Ok(readings_entry.readings)
    }

    pub fn len(&self) -> u64 {
        let prefix = match self.current_prefix.lock().unwrap().clone() {
            Some(prefix) => prefix,
            None => return 0,
        };

        let fst_map = self.fst_map.lock().unwrap();
        fst_map.get_link_for_key_dedup(&prefix).count() as u64
    }
}
