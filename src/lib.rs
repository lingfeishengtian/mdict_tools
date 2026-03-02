uniffi::setup_scaffolding!();

pub mod format;
pub mod mdict;

pub mod seekable_mmap;

pub mod error;
pub mod mdict_file;
pub mod mdict_optimized;
pub mod mdx_conversion;
pub mod packed_storage;
pub mod prefix_key_block_index;
pub mod random_access_key_blocks;
pub mod types;

pub use mdict::Mdict;
pub use mdict_file::MdictBundle;
pub use mdict_optimized::MdictOptimized;
