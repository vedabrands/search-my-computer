pub mod chunker;
pub mod extractor;
pub mod extractors;

pub use chunker::Chunk;
pub use chunker::code::CodeChunker;
pub use chunker::prose::ProseChunker;
pub use extractor::{ExtractedDoc, ExtractionLimits, Extractor, ExtractorRegistry, TextBlock};

use rusqlite::OptionalExtension;
use smc_core::db::Database;
use std::path::Path;
use tracing::{info, warn};

/// High-level function to extract content and chunk a file in one step.
pub fn extract_and_chunk(
    registry: &ExtractorRegistry,
    path: &Path,
    limits: &ExtractionLimits,
) -> Result<(ExtractedDoc, Vec<Chunk>), String> {
    let doc = registry.extract_file(path, limits)?;

    // Choose chunker based on document type
    let chunks = if let Some(lang) = &doc.language_guess {
        let code_chunker = CodeChunker::default();
        let full_text = doc.full_text();
        code_chunker.chunk_code(path, &full_text, Some(lang))
    } else {
        let prose_chunker = ProseChunker::default();
        prose_chunker.chunk_doc(&doc)
    };

    Ok((doc, chunks))
}

/// Process an extraction job for a specific file_id, persisting chunks to SQLite.
pub fn process_extract_job(
    db: &Database,
    file_id: i64,
    registry: &ExtractorRegistry,
    limits: &ExtractionLimits,
) -> Result<(), String> {
    let conn = db.reader().map_err(|e| format!("db pool error: {e}"))?;

    let file_info: Option<(String, String)> = conn
        .query_row(
            "SELECT path, status FROM files WHERE id = ?1",
            rusqlite::params![file_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| format!("db query error: {e}"))?;

    let (path_str, status) = match file_info {
        Some(info) => info,
        None => return Ok(()), // File was removed from DB
    };

    if status == "deleted" {
        return Ok(());
    }

    let path = Path::new(&path_str);
    if !path.exists() {
        let writer = db.writer();
        let _ = writer.execute(
            "UPDATE files SET status = 'error', error = 'file not found on disk' WHERE id = ?1",
            rusqlite::params![file_id],
        );
        return Err(format!("file not found on disk: {path_str}"));
    }

    let (doc, chunks) = match extract_and_chunk(registry, path, limits) {
        Ok(res) => res,
        Err(err) => {
            warn!(path = %path.display(), error = %err, "extraction failed");
            let writer = db.writer();
            let _ = writer.execute(
                "UPDATE files SET status = 'error', error = ?1 WHERE id = ?2",
                rusqlite::params![err, file_id],
            );
            return Err(err);
        }
    };

    let mut writer = db.writer();
    let tx = writer
        .transaction()
        .map_err(|e| format!("failed to start tx: {e}"))?;

    tx.execute(
        "DELETE FROM chunks WHERE file_id = ?1",
        rusqlite::params![file_id],
    )
    .map_err(|e| format!("failed to clear old chunks: {e}"))?;

    for chunk in &chunks {
        tx.execute(
            "INSERT INTO chunks (file_id, ordinal, text, page, section, symbol, start, end)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                file_id,
                chunk.ordinal as i64,
                chunk.text,
                chunk.page.map(|p| p as i64),
                chunk.section,
                chunk.symbol,
                chunk.start_offset as i64,
                chunk.end_offset as i64,
            ],
        )
        .map_err(|e| format!("failed to insert chunk: {e}"))?;
    }

    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE files SET status = 'active', error = NULL, last_indexed_at = ?1 WHERE id = ?2",
        rusqlite::params![now, file_id],
    )
    .map_err(|e| format!("failed to update file status: {e}"))?;

    // Enqueue embed job for this file if chunks were produced
    if !chunks.is_empty() {
        let _ = tx.execute(
            "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
             SELECT ?1, ?2, 3, ?3, ?4
             WHERE NOT EXISTS (
                 SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
             )",
            rusqlite::params![
                smc_core::schema::job_kind::EMBED,
                file_id,
                smc_core::schema::job_state::PENDING,
                now,
            ],
        );
    } else if doc.needs_ocr {
        // If document produced no text and flagged needs_ocr, queue low-priority vision job
        let _ = tx.execute(
            "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
             SELECT ?1, ?2, 1, ?3, ?4
             WHERE NOT EXISTS (
                 SELECT 1 FROM jobs WHERE file_id = ?2 AND kind = ?1 AND state IN ('pending', 'running')
             )",
            rusqlite::params![
                smc_core::schema::job_kind::VISION,
                file_id,
                smc_core::schema::job_state::PENDING,
                now,
            ],
        );
    }

    tx.commit()
        .map_err(|e| format!("failed to commit tx: {e}"))?;

    info!(
        file_id = file_id,
        path = %path.display(),
        chunks_count = chunks.len(),
        needs_ocr = doc.needs_ocr,
        "successfully extracted and chunked file"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use smc_core::db::Database;
    use std::fs;
    use std::io::Write;
    use std::time::Instant;
    use tempfile::tempdir;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    #[test]
    fn test_measure_extraction_throughput_and_db_growth() {
        let tmp = tempdir().unwrap();
        let registry = ExtractorRegistry::new();
        let limits = ExtractionLimits::default();

        println!("\n=== SMC-EXTRACT EXTRACTION THROUGHPUT BENCHMARK ===");

        // 1. Plain Text / Markdown (~50 KB)
        let txt_path = tmp.path().join("sample.txt");
        let txt_content = "The quick brown fox jumps over the lazy dog. Continuous semantic search for desktop.\n".repeat(800);
        fs::write(&txt_path, &txt_content).unwrap();

        let start = Instant::now();
        let iterations = 50;
        let mut total_chunks = 0;
        for _ in 0..iterations {
            let (_, chunks) = extract_and_chunk(&registry, &txt_path, &limits).unwrap();
            total_chunks += chunks.len();
        }
        let elapsed = start.elapsed();
        let total_bytes = txt_content.len() * iterations;
        let mb_per_sec = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();
        println!(
            "Plain Text / Markdown: {:.2} MB/s ({} chunks in {:?})",
            mb_per_sec, total_chunks, elapsed
        );

        // 2. JSON (~100 KB)
        let json_path = tmp.path().join("data.json");
        let json_records: Vec<serde_json::Value> = (0..500)
            .map(|i| serde_json::json!({ "id": i, "title": format!("Document record {}", i), "body": "Structured indexing test payload for laptop local search." }))
            .collect();
        let json_content = serde_json::to_string_pretty(&json_records).unwrap();
        fs::write(&json_path, &json_content).unwrap();

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = extract_and_chunk(&registry, &json_path, &limits).unwrap();
        }
        let elapsed = start.elapsed();
        let total_bytes = json_content.len() * iterations;
        let mb_per_sec = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();
        println!(
            "JSON format:          {:.2} MB/s in {:?}",
            mb_per_sec, elapsed
        );

        // 3. CSV (~100 KB)
        let csv_path = tmp.path().join("records.csv");
        let mut csv_content = String::from("id,name,role,department,location\n");
        for i in 0..2000 {
            csv_content.push_str(&format!(
                "{i},User{i},Engineer,Systems Engineering,Singapore\n"
            ));
        }
        fs::write(&csv_path, &csv_content).unwrap();

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = extract_and_chunk(&registry, &csv_path, &limits).unwrap();
        }
        let elapsed = start.elapsed();
        let total_bytes = csv_content.len() * iterations;
        let mb_per_sec = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();
        println!(
            "CSV format:           {:.2} MB/s in {:?}",
            mb_per_sec, elapsed
        );

        // 4. Code (Rust / Tree-sitter AST)
        let code_path = tmp.path().join("code.rs");
        let mut code_content = String::new();
        for i in 0..50 {
            code_content.push_str(&format!(
                "pub struct ServiceWorker{} {{\n    id: usize,\n}}\n\nimpl ServiceWorker{} {{\n    pub fn handle_event(&self) -> bool {{\n        true\n    }}\n}}\n\n",
                i, i
            ));
        }
        fs::write(&code_path, &code_content).unwrap();

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = extract_and_chunk(&registry, &code_path, &limits).unwrap();
        }
        let elapsed = start.elapsed();
        let total_bytes = code_content.len() * iterations;
        let mb_per_sec = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();
        println!(
            "Code (Rust / AST):    {:.2} MB/s in {:?}",
            mb_per_sec, elapsed
        );

        // 5. DOCX (Zip + XML)
        let docx_path = tmp.path().join("doc.docx");
        {
            let file = fs::File::create(&docx_path).unwrap();
            let mut zip = ZipWriter::new(file);
            let options = SimpleFileOptions::default();
            zip.start_file("word/document.xml", options).unwrap();
            let mut xml = String::from("<w:document><w:body>");
            for i in 0..100 {
                xml.push_str(&format!("<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Section {}</w:t></w:r></w:p>", i));
                xml.push_str("<w:p><w:r><w:t>Paragraph text detailing local search system design and performance benchmarks.</w:t></w:r></w:p>");
            }
            xml.push_str("</w:body></w:document>");
            zip.write_all(xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = extract_and_chunk(&registry, &docx_path, &limits).unwrap();
        }
        let elapsed = start.elapsed();
        let docx_size = fs::metadata(&docx_path).unwrap().len() as usize;
        let total_bytes = docx_size * iterations;
        let mb_per_sec = (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();
        println!(
            "DOCX (OpenXML):       {:.2} MB/s in {:?}",
            mb_per_sec, elapsed
        );

        // 6. DB Size Growth Measurement
        let db_path = tmp.path().join("bench_growth.db");
        let db = Database::open(&db_path).unwrap();

        let num_files = 1000;
        {
            let conn = db.writer();
            let tx = conn.unchecked_transaction().unwrap();

            for file_idx in 1..=num_files {
                tx.execute(
                    "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                     VALUES (?1, ?2, '/benchmark', ?3, 'txt', 2048, datetime('now'), datetime('now'), 'text', 'active', datetime('now'))",
                    (file_idx, format!("/benchmark/doc_{file_idx}.txt"), format!("doc_{file_idx}.txt")),
                ).unwrap();

                // 5 chunks per file on average = 5,000 chunks total
                for chunk_idx in 0..5 {
                    let cid = (file_idx - 1) * 5 + chunk_idx + 1;
                    let chunk_text = format!(
                        "Chunk {} for document {}. Local semantic search on desktop laptop with zero network calls and full privacy.",
                        chunk_idx, file_idx
                    );
                    tx.execute(
                        "INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                         VALUES (?1, ?2, ?3, ?4, 1, 'Main', NULL, 0, 100)",
                        (cid, file_idx, chunk_idx, chunk_text),
                    ).unwrap();
                }
            }

            tx.commit().unwrap();
        }

        let db_size_bytes = fs::metadata(&db_path).unwrap().len();
        let db_size_kb = db_size_bytes as f64 / 1024.0;
        let per_1k_files_kb = db_size_kb;
        let projected_per_100k_mb = (per_1k_files_kb * 100.0) / 1024.0;

        println!("\n=== DB GROWTH MEASUREMENT ===");
        println!(
            "DB size for 1,000 files (5,000 chunks + FTS5 full-text index): {:.2} KB ({:.2} MB)",
            db_size_kb,
            db_size_kb / 1024.0
        );
        println!(
            "Projected DB size for 100,000 indexed documents (500,000 chunks): {:.2} MB",
            projected_per_100k_mb
        );
        println!("===================================================\n");
    }
}
