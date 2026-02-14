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
