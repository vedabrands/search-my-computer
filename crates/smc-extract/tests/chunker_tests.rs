use smc_extract::chunker::code::CodeChunker;
use smc_extract::chunker::prose::{ProseChunker, ProseChunkerConfig};
use smc_extract::extractor::{ExtractedDoc, TextBlock};
use std::collections::HashMap;
use std::path::Path;

#[test]
fn test_prose_chunker_basic() {
    let doc = ExtractedDoc {
        title: Some("Sample Doc".to_string()),
        blocks: vec![
            TextBlock::new(
                "First block of text. Contains a couple sentences.",
                Some(1),
                Some("Introduction".to_string()),
                0,
                50,
            ),
            TextBlock::new(
                "Second block of text. Also contains some words.",
                Some(1),
                Some("Body".to_string()),
                52,
                100,
            ),
        ],
        metadata: HashMap::new(),
        language_guess: None,
        needs_ocr: false,
    };

    let chunker = ProseChunker::default();
    let chunks = chunker.chunk_doc(&doc);

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].ordinal, 0);
    assert_eq!(chunks[0].page, Some(1));
    assert_eq!(chunks[0].section, Some("Introduction".to_string()));
    assert_eq!(chunks[1].ordinal, 1);
    assert_eq!(chunks[1].section, Some("Body".to_string()));
}

#[test]
fn test_prose_chunker_large_block_overlap() {
    // Construct a long text with multiple sentences that exceeds 1000 characters
    let sentence = "This is an important sentence explaining system architecture. ";
    let mut long_text = String::new();
    for _ in 0..30 {
        long_text.push_str(sentence);
    }

    let doc = ExtractedDoc {
        title: Some("Large Document".to_string()),
        blocks: vec![TextBlock::new(
            &long_text,
            Some(1),
            Some("Section 1".to_string()),
            0,
            long_text.len(),
        )],
        metadata: HashMap::new(),
        language_guess: None,
        needs_ocr: false,
    };

    let config = ProseChunkerConfig {
        target_chars: 500,
        overlap_chars: 100,
        min_chunk_chars: 50,
    };
    let chunker = ProseChunker::new(config);
    let chunks = chunker.chunk_doc(&doc);

    assert!(chunks.len() > 1, "Expected multiple chunks for large text");
    // Verify chunk ordinals are consecutive
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.ordinal, i);
        assert_eq!(chunk.page, Some(1));
        assert_eq!(chunk.section, Some("Section 1".to_string()));
    }
}

#[test]
fn test_code_chunker_rust() {
    let code = r#"
pub struct UserConfig {
    pub name: String,
    pub max_items: usize,
}

impl UserConfig {
    pub fn new(name: String) -> Self {
        Self {
            name,
            max_items: 100,
        }
    }

    pub fn validate(&self) -> bool {
        !self.name.is_empty()
    }
}

pub fn top_level_helper(x: i32) -> i32 {
    x * 2
}
"#;

    let chunker = CodeChunker::default();
    let path = Path::new("src/config.rs");
    let chunks = chunker.chunk_code(path, code, Some("rust"));

    assert!(!chunks.is_empty());
    // Verify that symbol names are extracted (UserConfig, top_level_helper, impl, etc.)
    let symbol_names: Vec<String> = chunks.iter().filter_map(|c| c.symbol.clone()).collect();
    assert!(
        symbol_names
            .iter()
            .any(|s| s == "UserConfig" || s == "top_level_helper" || s.contains("impl")),
        "Extracted symbols: {:?}",
        symbol_names
    );

    // Verify header format
    assert!(
        chunks[0]
            .text
            .starts_with("// File: src/config.rs | Symbol: ")
    );
}

#[test]
fn test_code_chunker_python() {
    let py_code = r#"
class DataAggregator:
    def __init__(self, name):
        self.name = name
        self.data = []

    def add(self, item):
        self.data.append(item)

def calculate_metrics(values):
    return sum(values) / len(values) if values else 0
"#;

    let chunker = CodeChunker::default();
    let path = Path::new("scripts/analytics.py");
    let chunks = chunker.chunk_code(path, py_code, Some("python"));

    assert!(!chunks.is_empty());
    let symbol_names: Vec<String> = chunks.iter().filter_map(|c| c.symbol.clone()).collect();
    assert!(
        symbol_names
            .iter()
            .any(|s| s == "DataAggregator" || s == "calculate_metrics"),
        "Extracted python symbols: {:?}",
        symbol_names
    );
}

#[test]
fn test_code_chunker_typescript() {
    let ts_code = r#"
export interface Item {
    id: string;
    score: number;
}

export class SearchEngine {
    private items: Item[] = [];

    public search(query: string): Item[] {
        return this.items.filter(i => i.id.includes(query));
    }
}

export function formatResult(item: Item): string {
    return `[${item.id}] ${item.score}`;
}
"#;

    let chunker = CodeChunker::default();
    let path = Path::new("src/search.ts");
    let chunks = chunker.chunk_code(path, ts_code, Some("typescript"));

    assert!(!chunks.is_empty());
    let symbol_names: Vec<String> = chunks.iter().filter_map(|c| c.symbol.clone()).collect();
    assert!(
        symbol_names
            .iter()
            .any(|s| s == "SearchEngine" || s == "formatResult" || s == "Item"),
        "Extracted typescript symbols: {:?}",
        symbol_names
    );
}

#[test]
fn test_code_chunker_fallback() {
    // For unsupported language or flat non-AST files
    let mut flat_lines = String::new();
    for i in 1..=120 {
        flat_lines.push_str(&format!("SET VARIABLE_{i} = {i};\n"));
    }

    let chunker = CodeChunker::default();
    let path = Path::new("scripts/setup.customlang");
    let chunks = chunker.chunk_code(path, &flat_lines, None);

    assert!(chunks.len() >= 2, "Expected fallback line-windowed chunks");
    assert!(
        chunks[0]
            .text
            .contains("// File: scripts/setup.customlang (lines 1-50)")
    );
}
