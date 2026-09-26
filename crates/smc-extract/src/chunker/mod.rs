use serde::{Deserialize, Serialize};

pub mod code;
pub mod prose;

/// A chunk of text prepared for indexing and search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub ordinal: usize,
    pub text: String,
    pub page: Option<usize>,
    pub section: Option<String>,
    pub symbol: Option<String>,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl Chunk {
    pub fn new(
        ordinal: usize,
        text: impl Into<String>,
        page: Option<usize>,
        section: Option<String>,
        symbol: Option<String>,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        Self {
            ordinal,
            text: text.into(),
            page,
            section,
            symbol,
            start_offset,
            end_offset,
        }
    }
}
