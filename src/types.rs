/// Small domain types for the new public API. Keep these minimal for the scaffold.
#[derive(Debug, Clone)]
pub struct KeyBlock {
    pub key_id: u64,
    pub key_text: String,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub key: KeyBlock,
    pub record: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdictVersion {
    V1,
    V2,
    V3,
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
        }
    }

    /// Number of bytes used for index pairs / integer sizes in record/index headers.
    /// For V1 this is 4 (u32), for V2 this is 8 (u64).
    pub fn index_pair_size_bytes(&self) -> usize {
        match self {
            MdictVersion::V1 => 4usize,
            MdictVersion::V2 => 8usize,
            MdictVersion::V3 => panic!("Unsupported version for index sizes"),
        }
    }

    /// Size of the length fields for first/last key in key-info blocks.
    /// V1 uses 1-byte lengths, V2 uses 2-byte lengths.
    pub fn key_first_last_len_bytes(&self) -> usize {
        match self {
            MdictVersion::V1 => 1usize,
            MdictVersion::V2 => 2usize,
            MdictVersion::V3 => panic!("Unsupported version for key-info fields"),
        }
    }
}
