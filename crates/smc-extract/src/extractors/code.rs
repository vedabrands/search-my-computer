use crate::extractor::{ExtractedDoc, ExtractionLimits, Extractor, TextBlock};
use std::collections::HashMap;
use std::path::Path;

pub struct CodeExtractor;

impl Extractor for CodeExtractor {
    fn supported_extensions(&self) -> &[&str] {
        &[
            "rs", "py", "js", "ts", "jsx", "tsx", "c", "h", "cpp", "hpp", "cc", "cxx", "java",
            "cs", "go",
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
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            cow_str.into_owned()
        };

        let language_guess = Self::extension_to_language(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default(),
        );

        let mut blocks = Vec::new();
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            let len = content.len();
            blocks.push(TextBlock::new(content.clone(), None, None, 0, len));
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
        if let Some(ref lang) = language_guess {
            metadata.insert("language".to_string(), lang.clone());
        }

        Ok(ExtractedDoc {
            title: Some(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ),
            blocks,
            metadata,
            language_guess,
            needs_ocr: false,
        })
    }
}

impl CodeExtractor {
    fn extension_to_language(ext: &str) -> Option<String> {
        match ext.to_lowercase().as_str() {
            "rs" => Some("rust".to_string()),
            "py" => Some("python".to_string()),
            "js" | "jsx" => Some("javascript".to_string()),
            "ts" | "tsx" => Some("typescript".to_string()),
            "c" | "h" => Some("c".to_string()),
            "cpp" | "hpp" | "cc" | "cxx" => Some("cpp".to_string()),
            "java" => Some("java".to_string()),
            "cs" => Some("c_sharp".to_string()),
            "go" => Some("go".to_string()),
            _ => None,
        }
    }
}
