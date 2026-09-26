use crate::extractor::{ExtractedDoc, ExtractionLimits, Extractor, TextBlock};
use pdfium_render::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use tracing::warn;

pub struct PdfExtractor;

impl PdfExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PdfExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl Extractor for PdfExtractor {
    fn supported_extensions(&self) -> &[&str] {
        &["pdf"]
    }

    fn extract(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String> {
        let bindings = Pdfium::bind_to_system_library()
            .ok()
            .or_else(|| {
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./")).ok()
            })
            .or_else(|| {
                let exe = std::env::current_exe().ok()?;
                let dir = exe.parent()?;
                Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir)).ok()
            })
            .or_else(|| {
                let path_var = std::env::var("PDFIUM_PATH").ok()?;
                Pdfium::bind_to_library(path_var).ok()
            });

        let pdfium = bindings.map(Pdfium::new);

        let pdfium = match pdfium {
            Some(p) => p,
            None => {
                // If Pdfium binary is not available on this platform, flag needs_ocr / fallback
                warn!(path = %path.display(), "Pdfium library not available, skipping text extraction");
                return Ok(ExtractedDoc {
                    title: Some(
                        path.file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    ),
                    blocks: Vec::new(),
                    metadata: HashMap::new(),
                    language_guess: None,
                    needs_ocr: true,
                });
            }
        };

        let document = pdfium
            .load_pdf_from_byte_slice(bytes, None)
            .map_err(|e| format!("failed to load PDF: {e}"))?;

        let mut title = None;
        let mut metadata = HashMap::new();

        // Extract metadata tags if available
        if let Some(doc_title) = document.metadata().get(PdfDocumentMetadataTagType::Title) {
            let val = doc_title.value();
            if !val.trim().is_empty() {
                title = Some(val.trim().to_string());
            }
        }
        if let Some(author) = document.metadata().get(PdfDocumentMetadataTagType::Author) {
            let val = author.value();
            if !val.trim().is_empty() {
                metadata.insert("author".to_string(), val.trim().to_string());
            }
        }
        if let Some(creator) = document.metadata().get(PdfDocumentMetadataTagType::Creator) {
            let val = creator.value();
            if !val.trim().is_empty() {
                metadata.insert("creator".to_string(), val.trim().to_string());
            }
        }

        if title.is_none() {
            title = Some(
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }

        let mut blocks = Vec::new();
        let mut offset = 0;
        let page_count = document.pages().len() as usize;
        metadata.insert("page_count".to_string(), page_count.to_string());

        let max_pages = page_count.min(limits.max_pages);

        for page_idx in 0..max_pages {
            let text_opt = document
                .pages()
                .get(page_idx as u16)
                .ok()
                .and_then(|page| page.text().ok().map(|tp| tp.all()));

            if let Some(text) = text_opt {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let start = offset;
                    let end = offset + trimmed.len();
                    offset = end + 2;

                    blocks.push(TextBlock::new(
                        trimmed,
                        Some(page_idx + 1),
                        Some(format!("Page {}", page_idx + 1)),
                        start,
                        end,
                    ));
                }
            }
        }

        // If no text was found in any pages, mark as needs_ocr = true
        let needs_ocr = blocks.is_empty();

        Ok(ExtractedDoc {
            title,
            blocks,
            metadata,
            language_guess: None,
            needs_ocr,
        })
    }
}
