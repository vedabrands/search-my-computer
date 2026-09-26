use crate::extractor::{ExtractedDoc, ExtractionLimits, Extractor, TextBlock};
use calamine::{Data, Reader, open_workbook_auto_from_rs};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

pub struct XlsxExtractor;

impl Extractor for XlsxExtractor {
    fn supported_extensions(&self) -> &[&str] {
        &["xlsx", "xlsm", "xlsb", "xls", "ods"]
    }

    fn extract(
        &self,
        path: &Path,
        bytes: &[u8],
        limits: &ExtractionLimits,
    ) -> Result<ExtractedDoc, String> {
        let cursor = Cursor::new(bytes);
        let mut workbook = open_workbook_auto_from_rs(cursor)
            .map_err(|e| format!("failed to open spreadsheet: {e}"))?;

        let sheet_names = workbook.sheet_names().to_vec();
        let mut blocks = Vec::new();
        let mut offset = 0;
        let mut sheet_count = 0;

        for (sheet_idx, sheet_name) in sheet_names.iter().enumerate() {
            if sheet_idx >= limits.max_pages {
                break;
            }
            sheet_count += 1;

            if let Ok(range) = workbook.worksheet_range(sheet_name) {
                let mut sheet_text = String::new();
                for row in range.rows() {
                    let mut row_cells: Vec<String> = Vec::new();
                    for cell in row {
                        match cell {
                            Data::Empty => {}
                            Data::String(s) => {
                                let trimmed = s.trim();
                                if !trimmed.is_empty() {
                                    row_cells.push(trimmed.to_string());
                                }
                            }
                            Data::Float(f) => {
                                row_cells.push(f.to_string());
                            }
                            Data::Int(i) => {
                                row_cells.push(i.to_string());
                            }
                            Data::Bool(b) => {
                                row_cells.push(b.to_string());
                            }
                            Data::DateTime(dt) => {
                                row_cells.push(dt.to_string());
                            }
                            Data::DateTimeIso(dt) => {
                                row_cells.push(dt.to_string());
                            }
                            Data::DurationIso(dur) => {
                                row_cells.push(dur.to_string());
                            }
                            Data::Error(e) => {
                                row_cells.push(format!("#ERR:{e:?}"));
                            }
                        }
                    }

                    if !row_cells.is_empty() {
                        sheet_text.push_str(&row_cells.join("\t"));
                        sheet_text.push('\n');
                    }
                }

                let trimmed = sheet_text.trim();
                if !trimmed.is_empty() {
                    let start = offset;
                    let end = offset + trimmed.len();
                    offset = end + 2;

                    blocks.push(TextBlock::new(
                        trimmed,
                        Some(sheet_idx + 1),
                        Some(format!("Sheet: {sheet_name}")),
                        start,
                        end,
                    ));
                }
            }
        }

        let mut metadata = HashMap::new();
        metadata.insert("sheet_count".to_string(), sheet_count.to_string());
        metadata.insert("sheet_names".to_string(), sheet_names.join(", "));

        let title = Some(
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
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
