use crate::extractor::{ExtractedDoc, ExtractionLimits, Extractor, TextBlock};
use std::collections::HashMap;
use std::path::Path;

pub struct PlainTextExtractor;

impl Extractor for PlainTextExtractor {
    fn supported_extensions(&self) -> &[&str] {
        &[
            "txt", "text", "md", "markdown", "json", "csv", "tsv", "log", "yaml", "yml", "toml",
            "xml", "ini", "conf", "config", "env", "sql", "sh", "bat", "cmd", "ps1", "rst",
            "asciidoc", "adoc", "org", "tex", "latex", "rtf",
        ]
    }

    fn extract(
        &self,
        path: &Path,
        bytes: &[u8],
        _limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String> {
        let encoding = match encoding_rs::Encoding::for_bom(bytes) {
            Some((enc, _bom_len)) => enc,
            None => encoding_rs::UTF_8,
        };
        let (cow_str, _encoding_used, has_malformed) = encoding.decode(bytes);

        let content = if has_malformed {
            // Fallback to UTF-8 lossy if BOM decoder hit malformed bytes
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            cow_str.into_owned()
        };

        let mut title = None;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();

        // Title heuristics for markdown and text
        if ext == "md" || ext == "markdown" {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(h1) = trimmed.strip_prefix("# ") {
                    title = Some(h1.trim().to_string());
                    break;
                }
            }
        }

        let mut blocks = Vec::new();
        let mut offset = 0;

        // Split on double newlines to form paragraph blocks
        for para in content.split("\n\n") {
            let trimmed = para.trim();
            let start = offset;
            let end = offset + para.len();
            offset = end + 2;

            if !trimmed.is_empty() {
                blocks.push(TextBlock::new(trimmed, None, None, start, end));
            }
        }

        if blocks.is_empty() && !content.trim().is_empty() {
            let len = content.len();
            blocks.push(TextBlock::new(content.trim(), None, None, 0, len));
        }

        let mut metadata = HashMap::new();
        metadata.insert(
            "char_count".to_string(),
            content.chars().count().to_string(),
        );
        metadata.insert(
            "line_count".to_string(),
            content.lines().count().to_string(),
        );

        Ok(ExtractedDoc {
            title,
            blocks,
            metadata,
            language_guess: None,
            needs_ocr: false,
        })
    }
}
