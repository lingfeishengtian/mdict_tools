/// Small domain types for the new public API. Keep these minimal for the scaffold.
#[derive(Debug, Clone, uniffi::Record)]
pub struct KeyBlock {
    pub key_id: u64,
    pub key_text: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchHit {
    pub key: KeyBlock,
    pub record: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PrefixSearchCursor {
    pub after_key: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PrefixSearchPage {
    pub results: Vec<KeyBlock>,
    pub next_cursor: Option<PrefixSearchCursor>,
    pub total_results: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BuildProgressStage {
    Start,
    BuildReadings,
    BuildFst,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MdictVersion {
    V1,
    V2,
    V3,
    MDD,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Encoding {
    Utf8,
    Utf16LE,
    Unknown,
}

impl Default for MdictVersion {
    fn default() -> Self {
        MdictVersion::V1
    }
}

impl MdictVersion {
    pub fn major(&self) -> u8 {
        match self {
            MdictVersion::V1 => 1,
            MdictVersion::V2 => 2,
            MdictVersion::V3 => 3,
            MdictVersion::MDD => 2,
        }
    }

    /// Number of bytes used for index pairs / integer sizes in record/index headers.
    /// For V1 this is 4 (u32), for V2 this is 8 (u64).
    pub fn index_pair_size_bytes(&self) -> usize {
        match self {
            MdictVersion::V1 => 4usize,
            MdictVersion::V2 => 8usize,
            MdictVersion::V3 => panic!("Unsupported version for index sizes"),
            MdictVersion::MDD => 8usize,
        }
    }

    /// Size of the length fields for first/last key in key-info blocks.
    /// V1 uses 1-byte lengths, V2 uses 2-byte lengths.
    pub fn key_first_last_len_bytes(&self) -> usize {
        match self {
            MdictVersion::V1 => 1usize,
            MdictVersion::V2 => 2usize,
            MdictVersion::V3 => panic!("Unsupported version for key-info fields"),
            MdictVersion::MDD => 2usize,
        }
    }

    pub fn key_text_null_width(&self) -> usize {
        match self {
            MdictVersion::V1 => 1usize,
            MdictVersion::V2 => 1usize,
            MdictVersion::V3 => panic!("Unsupported version for key block fields"),
            MdictVersion::MDD => 2usize,
        }
    }
}

impl Encoding {
    /// Number of bytes per character unit for this encoding.
    /// UTF-8 => 1, UTF-16LE => 2. Unknown defaults to 2 for MDD-like handling.
    pub fn char_width(&self) -> usize {
        match self {
            Encoding::Utf8 => 1usize,
            Encoding::Utf16LE => 2usize,
            Encoding::Unknown => 2usize,
        }
    }
}
