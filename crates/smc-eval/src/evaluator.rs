use crate::queries::QuerySuite;
use serde::{Deserialize, Serialize};
use smc_core::config::AppConfig;
use smc_core::db::Database;
use smc_core::error::CoreResult;
use smc_core::scanner::Scanner;
use smc_embed::embedder::Embedder;
use smc_embed::processor::process_file_embedding;
use smc_embed::vector_index::VectorIndex;
use smc_extract::extractor::{ExtractionLimits, ExtractorRegistry};
use smc_extract::process_extract_job;
use smc_search::hybrid::hybrid_search_full;
use smc_search::ranking::RankingConfig;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryMetrics {
    pub category: String,
    pub count: usize,
    pub mrr: f64,
    pub recall_at_1: f64,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub avg_latency_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResultDetail {
    pub id: String,
    pub query: String,
    pub query_type: String,
    pub expected_files: Vec<String>,
    pub top_matches: Vec<String>,
    pub rank: Option<usize>,
    pub reciprocal_rank: f64,
    pub latency_ms: f64,
    pub hit_at_10: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub total_queries: usize,
    pub mrr: f64,
    pub recall_at_1: f64,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub avg_latency_ms: f64,
    pub by_category: HashMap<String, CategoryMetrics>,
    pub details: Vec<QueryResultDetail>,
}

impl EvalReport {
    pub fn print_summary_table(&self) {
        println!(
            "\n========================================================================================="
        );
        println!(
            "                         SEARCHMYCOMPUTER EVALUATION REPORT                              "
        );
        println!(
            "========================================================================================="
        );
        println!(
            "{:<16} | {:<7} | {:<8} | {:<9} | {:<9} | {:<10} | {:<12}",
            "Category", "Queries", "MRR", "Recall@1", "Recall@5", "Recall@10", "Avg Latency"
        );
        println!(
            "-----------------+---------+----------+-----------+-----------+------------+-------------"
        );

        // Print per-category rows sorted alphabetically
        let mut cats: Vec<&CategoryMetrics> = self.by_category.values().collect();
        cats.sort_by(|a, b| a.category.cmp(&b.category));

        for cat in cats {
            println!(
                "{:<16} | {:<7} | {:<8.4} | {:<8.2}% | {:<8.2}% | {:<9.2}% | {:<8.2} ms",
                cat.category,
                cat.count,
                cat.mrr,
                cat.recall_at_1 * 100.0,
                cat.recall_at_5 * 100.0,
                cat.recall_at_10 * 100.0,
                cat.avg_latency_ms
            );
        }

        println!(
            "-----------------+---------+----------+-----------+-----------+------------+-------------"
        );
        println!(
            "{:<16} | {:<7} | {:<8.4} | {:<8.2}% | {:<8.2}% | {:<9.2}% | {:<8.2} ms",
            "OVERALL (TOTAL)",
            self.total_queries,
            self.mrr,
            self.recall_at_1 * 100.0,
            self.recall_at_5 * 100.0,
            self.recall_at_10 * 100.0,
            self.avg_latency_ms
        );
        println!(
            "=========================================================================================\n"
        );
    }
}

/// Indexes an entire corpus folder into a fresh or existing SQLite database.
pub fn index_corpus_directory(
    corpus_dir: &Path,
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
) -> CoreResult<usize> {
    info!(corpus = %corpus_dir.display(), "indexing corpus directory for evaluation");

    // 1. Scan directory and populate files table
    let config = AppConfig {
        exclusions: Vec::new(), // Allow scanning test/tempdir corpus
        ..Default::default()
    };
    let scanner = Scanner::new(db.clone(), &config)?;
    let snapshot = scanner.scan_folder(corpus_dir)?;
    info!(files_seen = snapshot.files_seen, "scan complete");

    // 2. Extract content & chunk all files
    let registry = ExtractorRegistry::new();
    let limits = ExtractionLimits::default();

    let conn = db.reader()?;
    let mut stmt = conn.prepare("SELECT id, path FROM files WHERE status != 'deleted'")?;
    let file_rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(Result::ok)
        .collect();

    info!(count = file_rows.len(), "extracting and chunking documents");
    for (file_id, _) in &file_rows {
        let _ = process_extract_job(db, *file_id, &registry, &limits);
    }

    // 3. Generate embeddings if embedder & vector index provided
    if let (Some(emb), Some(vec_idx)) = (embedder, vector_index) {
        info!("embedding document chunks with ONNX model");
        for (file_id, _) in &file_rows {
            let _ = process_file_embedding(db, *file_id, emb, vec_idx);
        }
    }

    Ok(file_rows.len())
}

/// Runs the evaluation suite against the database.
pub fn evaluate_suite(
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    suite: &QuerySuite,
    config: &RankingConfig,
) -> CoreResult<EvalReport> {
    let mut details = Vec::with_capacity(suite.queries.len());
    let mut cat_map: HashMap<String, Vec<QueryResultDetail>> = HashMap::new();

    let mut total_latency = 0.0;
    let mut total_reciprocal_rank = 0.0;
    let mut total_r1 = 0;
    let mut total_r5 = 0;
    let mut total_r10 = 0;

    for q in &suite.queries {
        let start = Instant::now();
        let results = hybrid_search_full(db, embedder, vector_index, &q.query, 10, config)?;
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        total_latency += elapsed_ms;

        let top_match_paths: Vec<String> = results.iter().map(|r| r.path.clone()).collect();

        // Determine first rank of relevant document
        let mut first_rank: Option<usize> = None;
        for (idx, res) in results.iter().enumerate() {
            let norm_res_path = res.path.replace('\\', "/");
            let is_match = q.expected_files.iter().any(|expected| {
                let norm_exp = expected.replace('\\', "/");
                norm_res_path.ends_with(&norm_exp) || res.name.eq_ignore_ascii_case(&norm_exp)
            });

            if is_match {
                first_rank = Some(idx + 1);
                break;
            }
        }

        let rr = match first_rank {
            Some(r) => 1.0 / (r as f64),
            None => 0.0,
        };

        total_reciprocal_rank += rr;
        if let Some(r) = first_rank {
            if r <= 1 {
                total_r1 += 1;
            }
            if r <= 5 {
                total_r5 += 1;
            }
            if r <= 10 {
                total_r10 += 1;
            }
        }

        let detail = QueryResultDetail {
            id: q.id.clone(),
            query: q.query.clone(),
            query_type: q.query_type.clone(),
            expected_files: q.expected_files.clone(),
            top_matches: top_match_paths,
            rank: first_rank,
            reciprocal_rank: rr,
            latency_ms: elapsed_ms,
            hit_at_10: first_rank.is_some(),
        };

        cat_map
            .entry(q.query_type.clone())
            .or_default()
            .push(detail.clone());
        details.push(detail);
    }

    let n = suite.queries.len().max(1);
    let overall_mrr = total_reciprocal_rank / (n as f64);
    let overall_r1 = (total_r1 as f64) / (n as f64);
    let overall_r5 = (total_r5 as f64) / (n as f64);
    let overall_r10 = (total_r10 as f64) / (n as f64);
    let overall_avg_latency = total_latency / (n as f64);

    let mut by_category = HashMap::new();
    for (cat_name, cat_details) in cat_map {
        let cn = cat_details.len().max(1);
        let cat_mrr = cat_details.iter().map(|d| d.reciprocal_rank).sum::<f64>() / (cn as f64);
        let cat_r1 = (cat_details
            .iter()
            .filter(|d| d.rank.is_some_and(|r| r <= 1))
            .count() as f64)
            / (cn as f64);
        let cat_r5 = (cat_details
            .iter()
            .filter(|d| d.rank.is_some_and(|r| r <= 5))
            .count() as f64)
            / (cn as f64);
        let cat_r10 = (cat_details
            .iter()
            .filter(|d| d.rank.is_some_and(|r| r <= 10))
            .count() as f64)
            / (cn as f64);
        let cat_latency = cat_details.iter().map(|d| d.latency_ms).sum::<f64>() / (cn as f64);

        by_category.insert(
            cat_name.clone(),
            CategoryMetrics {
                category: cat_name,
                count: cat_details.len(),
                mrr: cat_mrr,
                recall_at_1: cat_r1,
                recall_at_5: cat_r5,
                recall_at_10: cat_r10,
                avg_latency_ms: cat_latency,
            },
        );
    }

    Ok(EvalReport {
        total_queries: suite.queries.len(),
        mrr: overall_mrr,
        recall_at_1: overall_r1,
        recall_at_5: overall_r5,
        recall_at_10: overall_r10,
        avg_latency_ms: overall_avg_latency,
        by_category,
        details,
    })
}

/// Enforces the regression gate: fails if current MRR is lower than (baseline_mrr - max_drop).
pub fn check_regression(
    report: &EvalReport,
    baseline_mrr: f64,
    max_mrr_drop: f64,
) -> Result<(), String> {
    let allowed_floor = (baseline_mrr - max_mrr_drop).max(0.0);
    if report.mrr < allowed_floor {
        Err(format!(
            "REGRESSION DETECTED: Current MRR ({:.4}) is below the allowed threshold ({:.4} = baseline {:.4} - tolerance {:.4})",
            report.mrr, allowed_floor, baseline_mrr, max_mrr_drop
        ))
    } else {
        info!(
            current_mrr = report.mrr,
            baseline_mrr = baseline_mrr,
            "Regression gate check passed successfully"
        );
        Ok(())
    }
}
