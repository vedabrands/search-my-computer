//! Benchmark suite for SearchMyComputer (Chunk 2 measurements).
//!
//! Measures:
//! 1. Directory scanner throughput on 50,000 files.
//! 2. Incremental scan throughput on unchanged files.
//! 3. SQLite DB size per 100,000 files (including FTS5 trigram index).
//! 4. Filename search query latency (p50, p95, p99) across trigram & prefix queries.

use rusqlite::params;
use smc_core::config::AppConfig;
use smc_core::db::Database;
use smc_core::scanner::Scanner;
use smc_search::filename::search;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use tempfile::TempDir;

fn create_test_tree(root: &Path, total_files: usize) {
    println!("Creating synthetic directory tree with {total_files} files...");
    let start = Instant::now();
    let num_dirs = 100;
    let files_per_dir = total_files / num_dirs;

    let extensions = [
        "rs", "ts", "tsx", "js", "py", "md", "json", "pdf", "docx", "png",
    ];
    let name_prefixes = [
        "SearchEngine",
        "user_profile",
        "auth_controller",
        "database_migration",
        "README",
        "package",
        "Cargo",
        "invoice_2026_q1",
        "presentation_final",
        "screenshot",
        "ReportGenerator",
        "data_pipeline",
        "config_loader",
        "helper_utils",
        "AppLayout",
    ];

    for d in 0..num_dirs {
        let dir_path = root.join(format!("project_{:03}/sub_{:02}", d, d % 10));
        fs::create_dir_all(&dir_path).unwrap();

        for f in 0..files_per_dir {
            let prefix = name_prefixes[(d + f) % name_prefixes.len()];
            let ext = extensions[(d * 7 + f) % extensions.len()];
            let file_name = format!("{prefix}_{d}_{f}.{ext}");
            let file_path = dir_path.join(file_name);

            let mut file = File::create(&file_path).unwrap();
            let _ = write!(
                file,
                "Sample content for file {d}_{f} in synthetic benchmark."
            );
        }
    }
    println!("Created {total_files} files in {:?}", start.elapsed());
}

fn main() {
    println!("=================================================================");
    println!("  SearchMyComputer - Performance & Scale Benchmark (Chunk 2)   ");
    println!("=================================================================\n");

    let temp_dir = TempDir::new().unwrap();
    let bench_root = temp_dir.path().join("bench_tree");
    fs::create_dir_all(&bench_root).unwrap();

    let db_path = temp_dir.path().join("bench_index.db");
    let db = Database::open(&db_path).unwrap();

    // Use specific benchmark exclusions so temp directory isn't skipped by default rules.
    let config = AppConfig {
        exclusions: vec![
            "**/node_modules/**".into(),
            "**/.git/**".into(),
            "**/target/**".into(),
        ],
        ..Default::default()
    };

    // --- 1. Scan speed on 50k files ---
    const SCAN_FILE_COUNT: usize = 50_000;
    create_test_tree(&bench_root, SCAN_FILE_COUNT);

    let scanner = Scanner::new(db.clone(), &config).unwrap();
    println!("\n[1/4] Running initial scan on {SCAN_FILE_COUNT} files...");
    let scan_start = Instant::now();
    let snapshot = scanner.scan_folder(&bench_root).unwrap();
    let scan_elapsed = scan_start.elapsed();
    let files_per_sec = (snapshot.files_seen as f64) / scan_elapsed.as_secs_f64();

    println!("  -> Scan completed in: {:?}", scan_elapsed);
    println!("  -> Files seen: {}", snapshot.files_seen);
    println!("  -> Files indexed: {}", snapshot.files_indexed);
    println!("  -> Files skipped: {}", snapshot.files_skipped);
    println!("  -> Scan throughput: {:.1} files/sec", files_per_sec);

    // --- 2. Incremental scan speed ---
    println!("\n[2/4] Running incremental scan (unchanged files)...");
    let inc_start = Instant::now();
    let inc_snapshot = scanner.scan_folder(&bench_root).unwrap();
    let inc_elapsed = inc_start.elapsed();
    let inc_throughput = (inc_snapshot.files_seen as f64) / inc_elapsed.as_secs_f64();

    println!("  -> Incremental scan completed in: {:?}", inc_elapsed);
    println!("  -> Files seen: {}", inc_snapshot.files_seen);
    println!(
        "  -> Files skipped (unchanged): {}",
        inc_snapshot.files_skipped
    );
    println!(
        "  -> Incremental throughput: {:.1} files/sec",
        inc_throughput
    );

    // --- 3. Database scale up to 100,000 files & DB Size measurement ---
    println!("\n[3/4] Scaling database to 100,000 files for size measurement...");
    let now = chrono::Utc::now().to_rfc3339();
    {
        let mut conn = db.writer();
        let tx = conn.transaction().unwrap();
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO files (path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, 'active', ?6)",
                )
                .unwrap();

            let extensions = ["rs", "ts", "py", "md", "pdf", "docx", "json", "png"];
            for i in 50_000..100_000 {
                let ext = extensions[i % extensions.len()];
                let path = format!(
                    "/benchmark/scaled/repo_{}/source_file_{:06}.{}",
                    i / 1000,
                    i,
                    ext
                );
                let parent = format!("/benchmark/scaled/repo_{}", i / 1000);
                let name = format!("source_file_{:06}.{}", i, ext);
                stmt.execute(params![path, parent, name, ext, 4096, now, "code"])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
    }

    // Flush WAL to main database for accurate disk measurement.
    {
        let conn = db.writer();
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
    }

    let db_metadata = fs::metadata(&db_path).unwrap();
    let db_size_bytes = db_metadata.len();
    let db_size_mb = (db_size_bytes as f64) / (1024.0 * 1024.0);
    let bytes_per_file = (db_size_bytes as f64) / 100_000.0;

    println!("  -> Total files indexed in SQLite: 100,000");
    println!(
        "  -> SQLite DB file size: {:.2} MB ({} bytes)",
        db_size_mb, db_size_bytes
    );
    println!(
        "  -> Average size per indexed file (with FTS5 trigrams): {:.1} bytes/file",
        bytes_per_file
    );

    // --- 4. Query Latency Benchmark ---
    println!("\n[4/4] Measuring query latency over 100,000 indexed files...");
    let test_queries = [
        "SearchEngine",
        "auth",
        "controller",
        "README",
        "invoice",
        "pipeline",
        "source_file",
        "utils",
        "config",
        "user_profile",
        "layout",
        "presentation",
        "migration",
        "package",
        "Cargo",
        "Sea",
        "aut",
        "inv",
    ];

    // Warm up reader pool.
    for q in &test_queries {
        let _ = search(&db, q, 20).unwrap();
    }

    let mut latencies: Vec<f64> = Vec::new();
    let iterations = 1000;

    for i in 0..iterations {
        let query = test_queries[i % test_queries.len()];
        let t0 = Instant::now();
        let res = search(&db, query, 20).unwrap();
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        latencies.push(elapsed_ms);
        assert!(!res.is_empty(), "query '{query}' should return results");
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = latencies[0];
    let p50 = latencies[iterations * 50 / 100];
    let p90 = latencies[iterations * 90 / 100];
    let p95 = latencies[iterations * 95 / 100];
    let p99 = latencies[iterations * 99 / 100];
    let max = latencies[iterations - 1];
    let mean: f64 = latencies.iter().sum::<f64>() / (iterations as f64);

    println!("  -> Sample size: {} queries", iterations);
    println!("  -> Latency Mean: {:.3} ms", mean);
    println!("  -> Latency Min:  {:.3} ms", min);
    println!("  -> Latency p50:  {:.3} ms", p50);
    println!("  -> Latency p90:  {:.3} ms", p90);
    println!("  -> Latency p95:  {:.3} ms", p95);
    println!("  -> Latency p99:  {:.3} ms", p99);
    println!("  -> Latency Max:  {:.3} ms", max);

    println!("\n=================================================================");
    println!("  Benchmark Completed Successfully!                              ");
    println!("=================================================================\n");
}
