use crate::extractor::{ExtractedDoc, ExtractionLimits, Extractor, TextBlock};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

pub struct DocxExtractor;

impl Extractor for DocxExtractor {
    fn supported_extensions(&self) -> &[&str] {
        &["docx", "docm", "dotx", "dotm"]
    }

    fn extract(
        &self,
        path: &Path,
        bytes: &[u8],
        _limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String> {
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("failed to read docx zip archive: {e}"))?;

        // 1. Try to extract metadata and title from docProps/core.xml if present
        let mut title = None;
        let mut metadata = HashMap::new();

        if let Ok(mut core_file) = archive.by_name("docProps/core.xml") {
            let mut core_xml = String::new();
            if core_file.read_to_string(&mut core_xml).is_ok() {
                let (core_title, core_meta) = parse_core_properties(&core_xml);
                title = core_title;
                metadata.extend(core_meta);
            }
        }

        // 2. Extract text from word/document.xml
        let mut doc_file = archive
            .by_name("word/document.xml")
            .map_err(|e| format!("docx missing word/document.xml: {e}"))?;

        let mut doc_xml = String::new();
        doc_file
            .read_to_string(&mut doc_xml)
            .map_err(|e| format!("failed to read word/document.xml: {e}"))?;

        let mut blocks = Vec::new();
        let mut reader = Reader::from_str(&doc_xml);
        reader.config_mut().trim_text(false);

        let mut buf = Vec::new();
        let mut current_para = String::new();
        let mut in_text = false;
        let mut current_heading: Option<String> = None;
        let mut offset = 0;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == b"w:t" {
                        in_text = true;
                    } else if name.as_ref() == b"w:pStyle" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" {
                                let lower = String::from_utf8_lossy(&attr.value).to_lowercase();
                                if lower.starts_with("heading") || lower == "title" {
                                    current_heading =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == b"w:pStyle" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" {
                                let lower = String::from_utf8_lossy(&attr.value).to_lowercase();
                                if lower.starts_with("heading") || lower == "title" {
                                    current_heading =
                                        Some(String::from_utf8_lossy(&attr.value).to_string());
                                }
                            }
                        }
                    } else if name.as_ref() == b"w:tab" {
                        current_para.push('\t');
                    } else if name.as_ref() == b"w:br" || name.as_ref() == b"w:cr" {
                        current_para.push('\n');
                    }
                }
                Ok(Event::Text(ref e)) if in_text => {
                    if let Ok(text) = e.unescape() {
                        current_para.push_str(&text);
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name();
                    if name.as_ref() == b"w:t" {
                        in_text = false;
                    } else if name.as_ref() == b"w:p" {
                        let trimmed = current_para.trim();
                        if !trimmed.is_empty() {
                            let start = offset;
                            let end = offset + trimmed.len();
                            offset = end + 2;

                            if title.is_none() && current_heading.is_some() {
                                title = Some(trimmed.to_string());
                            }

                            blocks.push(TextBlock::new(
                                trimmed,
                                None,
                                current_heading.take(),
                                start,
                                end,
                            ));
                        }
                        current_para.clear();
                        current_heading = None;
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parsing error in docx: {e}")),
                _ => {}
            }
            buf.clear();
        }

        if title.is_none() {
            title = Some(
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }

        metadata.insert("block_count".to_string(), blocks.len().to_string());

        Ok(ExtractedDoc {
            title,
            blocks,
            metadata,
            language_guess: None,
            needs_ocr: false,
        })
    }
}

fn parse_core_properties(xml: &str) -> (Option<String>, HashMap<String, String>) {
    let mut title = None;
    let mut metadata = HashMap::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                current_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(text) = e.unescape() {
                    let val = text.trim().to_string();
                    if !val.is_empty() {
                        match current_tag.as_str() {
                            "dc:title" => title = Some(val.clone()),
                            "dc:creator" => {
                                metadata.insert("author".to_string(), val);
                            }
                            "cp:lastModifiedBy" => {
                                metadata.insert("last_modified_by".to_string(), val);
                            }
                            "cp:revision" => {
                                metadata.insert("revision".to_string(), val);
                            }
                            "dcterms:created" => {
                                metadata.insert("created".to_string(), val);
                            }
                            "dcterms:modified" => {
                                metadata.insert("modified".to_string(), val);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    (title, metadata)
}
