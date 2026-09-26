use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tracing::{debug, warn};

/// Resource and parsing constraints for extracting content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionLimits {
    /// Maximum file size to read into memory (bytes).
    pub max_bytes: usize,
    /// Maximum number of pages/slides/sheets to parse.
    pub max_pages: usize,
    /// Per-file extraction timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for ExtractionLimits {
    fn default() -> Self {
        Self {
            max_bytes: 50 * 1024 * 1024, // 50 MB
            max_pages: 1000,
            timeout_ms: 10_000, // 10 seconds
        }
    }
}

/// A contiguous block of text with spatial and structural context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
    pub page: Option<usize>,
    pub section: Option<String>,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl TextBlock {
    pub fn new(
        text: impl Into<String>,
        page: Option<usize>,
        section: Option<String>,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        Self {
            text: text.into(),
            page,
            section,
            start_offset,
            end_offset,
        }
    }
}

/// Structured document extracted from a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExtractedDoc {
    pub title: Option<String>,
    pub blocks: Vec<TextBlock>,
    pub metadata: HashMap<String, String>,
    pub language_guess: Option<String>,
    pub needs_ocr: bool,
}

impl ExtractedDoc {
    /// Return the combined text of all blocks joined with double newlines.
    pub fn full_text(&self) -> String {
        self.blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Check if the extracted document contains any non-whitespace text.
    pub fn is_empty(&self) -> bool {
        self.blocks.iter().all(|b| b.text.trim().is_empty())
    }
}

/// Common trait implemented by all format-specific extractors.
pub trait Extractor: Send + Sync {
    /// List of file extensions this extractor claims (without leading dot, lowercase).
    fn supported_extensions(&self) -> &[&str];

    /// Parse file content and produce an ExtractedDoc.
    fn extract(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String>;
}

/// Registry that dispatches files to appropriate extractors based on extension and content sniffing.
pub struct ExtractorRegistry {
    extractors: Vec<Box<dyn Extractor>>,
    extension_map: HashMap<String, usize>,
    plain_text_index: usize,
    code_index: usize,
}

impl ExtractorRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            extractors: Vec::new(),
            extension_map: HashMap::new(),
            plain_text_index: 0,
            code_index: 0,
        };

        // Register default extractors.
        registry.register(Box::new(crate::extractors::plain_text::PlainTextExtractor));
        registry.plain_text_index = registry.extractors.len() - 1;

        registry.register(Box::new(crate::extractors::code::CodeExtractor));
        registry.code_index = registry.extractors.len() - 1;

        registry.register(Box::new(crate::extractors::pdf::PdfExtractor::new()));
        registry.register(Box::new(crate::extractors::docx::DocxExtractor));
        registry.register(Box::new(crate::extractors::pptx::PptxExtractor));
        registry.register(Box::new(crate::extractors::xlsx::XlsxExtractor));

        registry
    }

    /// Register a new extractor.
    pub fn register(&mut self, extractor: Box<dyn Extractor>) {
        let index = self.extractors.len();
        for ext in extractor.supported_extensions() {
            self.extension_map.insert(ext.to_lowercase(), index);
        }
        self.extractors.push(extractor);
    }

    /// Check if a file extension is explicitly registered with an extractor.
    pub fn supports_extension(&self, ext: &str) -> bool {
        self.extension_map.contains_key(&ext.to_lowercase())
    }

    /// Extract document content from a file on disk.
    /// Handles file reading limits, content sniffing, and catches any panics.
    pub fn extract_file(
        &self,
        path: &Path,
        limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String> {
        let metadata = std::fs::metadata(path).map_err(|e| format!("cannot stat file: {e}"))?;
        if metadata.len() > limits.max_bytes as u64 {
            return Err(format!(
                "file size ({} bytes) exceeds limit ({} bytes)",
                metadata.len(),
                limits.max_bytes
            ));
        }

        let mut file = File::open(path).map_err(|e| format!("cannot open file: {e}"))?;
        let mut buffer = Vec::with_capacity(metadata.len().min(limits.max_bytes as u64) as usize);
        file.read_to_end(&mut buffer)
            .map_err(|e| format!("cannot read file: {e}"))?;

        self.extract_bytes(path, &buffer, limits)
    }

    /// Extract document content from a byte buffer.
    pub fn extract_bytes(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String> {
        if bytes.is_empty() {
            return Ok(ExtractedDoc::default());
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        let extractor_index = if let Some(&idx) = self.extension_map.get(&ext) {
            Some(idx)
        } else {
            // Content sniffing: check if the content is text or binary.
            let inspection = content_inspector::inspect(bytes);
            if inspection.is_text() {
                Some(self.plain_text_index)
            } else {
                debug!(path = %path.display(), "binary file with unknown extension, skipping content extraction");
                None
            }
        };

        let extractor_idx = match extractor_index {
            Some(idx) => idx,
            None => return Ok(ExtractedDoc::default()),
        };

        let extractor = &self.extractors[extractor_idx];

        // Isolate parser crashes/panics with catch_unwind.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            extractor.extract(path, bytes, limits)
        }));

        match result {
            Ok(res) => res,
            Err(panic_err) => {
                let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_err.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic in extractor".to_string()
                };
                warn!(path = %path.display(), error = %msg, "extractor panicked");
                Err(format!("extractor panic: {msg}"))
            }
        }
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
