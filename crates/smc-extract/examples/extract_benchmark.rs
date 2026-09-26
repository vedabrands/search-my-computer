use smc_core::db::Database;
use smc_extract::extract_and_chunk;
use smc_extract::extractor::{ExtractionLimits, ExtractorRegistry};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

#[derive(Default)]
struct CategoryStats {
    count: usize,
    total_bytes: u64,
    total_chunks: usize,
    duration_micros: u128,
    errors: usize,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let target_dir = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        println!(
            "No folder specified. Usage: cargo run -p smc-extract --example extract_benchmark -- <FOLDER_PATH>"
        );
        println!("Defaulting to scanning current workspace directory...\n");
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };

    if !target_dir.exists() {
        eprintln!(
            "Error: Target path '{}' does not exist.",
            target_dir.display()
        );
        std::process::exit(1);
    }

    println!("================================================================================");
    println!("        SMC-EXTRACT REAL-DOCUMENT BENCHMARK & THROUGHPUT MEASUREMENT            ");
    println!("================================================================================");
    println!("Target Directory: {}", target_dir.display());

    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits {
        max_bytes: 50 * 1024 * 1024, // 50 MB
        max_pages: 1000,
        timeout_ms: 30_000,
    };

    // 1. Discover all candidate files
    println!("Scanning directory for supported documents & code files...");
    let scan_start = Instant::now();
    let mut candidate_files: Vec<PathBuf> = Vec::new();

    for entry in WalkDir::new(&target_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if registry.supports_extension(&ext_lower) {
                    // Skip git internals, target folders, and node_modules
                    let path_str = path.to_string_lossy();
                    if !path_str.contains(".git")
                        && !path_str.contains("target")
                        && !path_str.contains("node_modules")
                    {
                        candidate_files.push(path.to_path_buf());
                    }
                }
            }
        }
    }

    let scan_elapsed = scan_start.elapsed();
    println!(
        "Found {} supported documents in {:.2?}.\n",
        candidate_files.len(),
        scan_elapsed
    );

    if candidate_files.is_empty() {
        println!("No supported files found in the specified directory.");
        return;
    }

    // 2. Set up a fresh SQLite database
    let temp_dir = tempfile::tempdir().expect("failed to create tempdir");
    let db_path = temp_dir.path().join("real_docs_benchmark.db");
    let db = Database::open(&db_path).expect("failed to open database");

    // 3. Extract and chunk all files
    println!("Running extraction, chunking, and SQLite FTS5 persistence...");
    let bench_start = Instant::now();
    let mut stats_by_category: HashMap<String, CategoryStats> = HashMap::new();
    let mut total_chunks_inserted = 0;
    let mut total_bytes_processed = 0u64;
    let mut failed_files: Vec<(String, String)> = Vec::new();

    let mut needs_ocr_files: Vec<String> = Vec::new();

    for (idx, path) in candidate_files.iter().enumerate() {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown")
            .to_lowercase();

        let category = match ext.as_str() {
            "pdf" => "PDF Documents",
            "docx" | "docm" | "dotx" | "dotm" => "DOCX Word Documents",
            "pptx" | "pptm" | "potx" | "potm" => "PPTX Presentations",
            "xlsx" | "xlsm" | "xltx" | "xltm" | "xls" | "ods" => "XLSX Spreadsheets",
            "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "c" | "cpp" | "h" | "hpp" | "java"
            | "cs" | "go" => "Source Code (AST)",
            "md" | "markdown" => "Markdown",
            "txt" | "log" | "ini" | "env" => "Plain Text / Logs",
            "json" | "csv" => "Structured (JSON/CSV)",
            _ => "Other Supported",
        };

        let file_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        total_bytes_processed += file_size;

        let file_start = Instant::now();
        let extract_res = extract_and_chunk(&registry, path, &limits);
        let file_elapsed = file_start.elapsed().as_micros();

        let stat = stats_by_category.entry(category.to_string()).or_default();
        stat.count += 1;
        stat.total_bytes += file_size;
        stat.duration_micros += file_elapsed;

        match extract_res {
            Ok((doc, chunks)) => {
                stat.total_chunks += chunks.len();
                total_chunks_inserted += chunks.len();

                if doc.needs_ocr {
                    needs_ocr_files.push(path.to_string_lossy().to_string());
                }

                // Persist to DB
                let file_id = (idx + 1) as i64;
                let file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let parent_dir = path
                    .parent()
                    .unwrap_or_else(|| Path::new("/"))
                    .to_string_lossy()
                    .to_string();

                let mut writer = db.writer();
                let tx = writer.transaction().expect("failed to start tx");

                let now = chrono::Utc::now().to_rfc3339();
                tx.execute(
                    "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 'doc', 'active', ?7)",
                    rusqlite::params![
                        file_id,
                        path.to_string_lossy(),
                        parent_dir,
                        file_name,
                        ext,
                        file_size as i64,
                        now,
                    ],
                ).expect("insert file failed");

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
                    ).expect("insert chunk failed");
                }

                tx.commit().expect("tx commit failed");
            }
            Err(err) => {
                stat.errors += 1;
                failed_files.push((path.to_string_lossy().to_string(), err));
            }
        }
    }

    // Checkpoint WAL so database file reflects full size on disk
    {
        let writer = db.writer();
        let _ = writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    let bench_elapsed = bench_start.elapsed();

    // 4. Print Results Breakdown
    println!("\n=== RESULTS BY DOCUMENT CATEGORY ===");
    println!(
        "{:<24} | {:>6} | {:>9} | {:>8} | {:>10} | {:>8} | {:>6}",
        "Category", "Files", "Size (MB)", "Time (s)", "Throughput", "Chunks", "Errors"
    );
    println!(
        "{:-<24}-+-{:-<6}-+-{:-<9}-+-{:-<8}-+-{:-<10}-+-{:-<8}-+-{:-<6}",
        "", "", "", "", "", "", ""
    );

    let mut sorted_categories: Vec<_> = stats_by_category.into_iter().collect();
    sorted_categories.sort_by_key(|a| std::cmp::Reverse(a.1.total_bytes));

    for (cat, stat) in sorted_categories {
        let size_mb = stat.total_bytes as f64 / (1024.0 * 1024.0);
        let time_sec = stat.duration_micros as f64 / 1_000_000.0;
        let mb_per_sec = if time_sec > 0.0 {
            size_mb / time_sec
        } else {
            0.0
        };

        println!(
            "{:<24} | {:>6} | {:>9.2} | {:>8.3} | {:>7.2} MB/s | {:>8} | {:>6}",
            cat, stat.count, size_mb, time_sec, mb_per_sec, stat.total_chunks, stat.errors
        );
    }

    let total_mb = total_bytes_processed as f64 / (1024.0 * 1024.0);
    let total_sec = bench_elapsed.as_secs_f64();
    let overall_mb_per_sec = if total_sec > 0.0 {
        total_mb / total_sec
    } else {
        0.0
    };

    println!("================================================================================");
    println!("Overall Extraction & Chunking Summary:");
    println!("  Total Files Processed:  {}", candidate_files.len());
    println!(
        "  Total Volume:           {:.2} MB ({} bytes)",
        total_mb, total_bytes_processed
    );
    println!("  Total Chunks Generated: {}", total_chunks_inserted);
    println!("  Total Wall-Clock Time:  {:.3?}", bench_elapsed);
    println!(
        "  Overall Throughput:     {:.2} MB/s ({:.1} files/sec)",
        overall_mb_per_sec,
        candidate_files.len() as f64 / total_sec
    );

    // 5. Database on-disk growth
    let db_main_bytes = fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
    let wal_path = db_path.with_extension("db-wal");
    let wal_bytes = fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);
    let shm_path = db_path.with_extension("db-shm");
    let shm_bytes = fs::metadata(&shm_path).map(|m| m.len()).unwrap_or(0);
    let db_size_bytes = db_main_bytes + wal_bytes + shm_bytes;

    let db_size_kb = db_size_bytes as f64 / 1024.0;
    let db_size_mb = db_size_bytes as f64 / (1024.0 * 1024.0);
    println!("\n=== SQLITE DATABASE DISK FOOTPRINT ===");
    println!(
        "  SQLite Database Size:   {:.2} KB ({:.2} MB) [main: {} B, wal: {} B, shm: {} B]",
        db_size_kb, db_size_mb, db_main_bytes, wal_bytes, shm_bytes,
    );
    if !candidate_files.is_empty() {
        let avg_kb_per_file = db_size_kb / candidate_files.len() as f64;
        println!(
            "  Average DB per File:    {:.2} KB ({:.0} bytes)",
            avg_kb_per_file,
            db_size_bytes as f64 / candidate_files.len() as f64
        );
        println!(
            "  Projected (100k files): {:.2} MB ({:.2} GB)",
            (avg_kb_per_file * 100_000.0) / 1024.0,
            (avg_kb_per_file * 100_000.0) / (1024.0 * 1024.0)
        );
    }
    if total_chunks_inserted > 0 {
        let avg_bytes_per_chunk = (db_size_bytes as f64) / total_chunks_inserted as f64;
        println!("  Average DB per Chunk:   {:.2} bytes", avg_bytes_per_chunk);
        println!(
            "  Projected (500k chunks):{:.2} MB ({:.2} GB)",
            (avg_bytes_per_chunk * 500_000.0) / (1024.0 * 1024.0),
            (avg_bytes_per_chunk * 500_000.0) / (1024.0 * 1024.0 * 1024.0)
        );
    }

    if !needs_ocr_files.is_empty() {
        println!(
            "\n=== NEEDS_OCR SCANNED DOCUMENTS ({} total) ===",
            needs_ocr_files.len()
        );
        for fpath in &needs_ocr_files {
            println!("  [needs_ocr=true] {}", fpath);
        }
    }

    // 6. Test FTS5 Queries on the real extracted data
    println!("\n=== TESTING FTS5 BM25 SEARCH QUERIES ON REAL DATA ===");
    let reader = db.reader().expect("failed to get reader");
    let test_queries = [
        "search", "function", "data", "report", "the", "system", "file", "error",
    ];

    for query in &test_queries {
        let q_start = Instant::now();
        let mut stmt = reader
            .prepare(
                "SELECT c.id, f.name, snippet(chunks_fts, 0, '<mark>', '</mark>', '...', 24)
                 FROM chunks_fts
                 JOIN chunks c ON c.id = chunks_fts.rowid
                 JOIN files f ON f.id = c.file_id
                 WHERE chunks_fts MATCH ?1
                 LIMIT 10",
            )
            .expect("prepare search query failed");

        let hits = stmt
            .query_map(rusqlite::params![format!("\"{query}\"")], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .expect("query map failed")
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        let q_elapsed = q_start.elapsed();
        println!(
            "  Query \"{:<10}\" -> {:>3} hits in {:>6.2?}",
            query,
            hits.len(),
            q_elapsed
        );
    }

    if !failed_files.is_empty() {
        println!("\n=== FAILED FILES ({} total) ===", failed_files.len());
        for (fpath, err) in failed_files.iter().take(10) {
            println!("  [FAIL] {} -> {}", fpath, err);
        }
        if failed_files.len() > 10 {
            println!("  ... and {} more files.", failed_files.len() - 10);
        }
    } else {
        println!("\nAll files extracted with 100% success rate (0 errors).");
    }

    println!("================================================================================");
}
