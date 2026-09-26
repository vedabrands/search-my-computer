use crate::extractor::{ExtractedDoc, ExtractionLimits, Extractor, TextBlock};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

pub struct PptxExtractor;

impl Extractor for PptxExtractor {
    fn supported_extensions(&self) -> &[&str] {
        &["pptx", "pptm", "potx", "potm"]
    }

    fn extract(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String> {
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("failed to read pptx zip archive: {e}"))?;

        // 1. Try to extract metadata from docProps/core.xml
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

        // 2. Discover slide files
        let mut slide_names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            if let Ok(file) = archive.by_index(i) {
                let name = file.name();
                if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                    slide_names.push(name.to_string());
                }
            }
        }

        // Sort slide files naturally by slide number (e.g., slide1.xml, slide2.xml ... slide10.xml)
        slide_names.sort_by_key(|name| {
            name.strip_prefix("ppt/slides/slide")
                .and_then(|s| s.strip_suffix(".xml"))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(usize::MAX)
        });

        let mut blocks = Vec::new();
        let mut offset = 0;
        let mut total_slides = 0;

        for (slide_idx, slide_name) in slide_names.iter().enumerate() {
            if slide_idx >= limits.max_pages {
                break;
            }
            total_slides += 1;

            let mut slide_file = match archive.by_name(slide_name) {
                Ok(f) => f,
                Err(_) => continue,
            };

            let mut slide_xml = String::new();
            if slide_file.read_to_string(&mut slide_xml).is_err() {
                continue;
            }

            let slide_blocks = parse_slide_xml(&slide_xml, slide_idx + 1, &mut offset);
            blocks.extend(slide_blocks);
        }

        if title.is_none() {
            // If title is missing from properties, try using first text block from slide 1
            if let Some(first_block) = blocks.first() {
                let first_line = first_block.text.lines().next().unwrap_or_default().trim();
                if !first_line.is_empty() {
                    title = Some(first_line.to_string());
                }
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

        metadata.insert("slide_count".to_string(), total_slides.to_string());

        Ok(ExtractedDoc {
            title,
            blocks,
            metadata,
            language_guess: None,
            needs_ocr: false,
        })
    }
}

fn parse_slide_xml(xml: &str, slide_num: usize, global_offset: &mut usize) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut current_para = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                if name.as_ref() == b"a:t" {
                    in_text = true;
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                if name.as_ref() == b"a:br" {
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
                if name.as_ref() == b"a:t" {
                    in_text = false;
                } else if name.as_ref() == b"a:p" {
                    let trimmed = current_para.trim();
                    if !trimmed.is_empty() {
                        let start = *global_offset;
                        let end = start + trimmed.len();
                        *global_offset = end + 2;

                        blocks.push(TextBlock::new(
                            trimmed,
                            Some(slide_num),
                            Some(format!("Slide {slide_num}")),
                            start,
                            end,
                        ));
                    }
                    current_para.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    blocks
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
