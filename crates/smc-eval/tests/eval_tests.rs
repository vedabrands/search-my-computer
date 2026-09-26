use smc_eval::corpus::generate_synthetic_corpus;
use smc_eval::evaluator::{EvalReport, check_regression};
use smc_eval::queries::QuerySuite;
use tempfile::tempdir;

#[test]
fn test_synthetic_corpus_generation_and_counts() {
    let tmp = tempdir().unwrap();
    let count = generate_synthetic_corpus(tmp.path()).unwrap();
    assert!(
        count >= 300,
        "Corpus should contain at least 300 files, generated {}",
        count
    );

    // Verify key files exist
    assert!(
        tmp.path()
            .join("finance/q3_financial_report_2024.pdf")
            .exists()
    );
    assert!(tmp.path().join("code/payment_gateway.ts").exists());
    assert!(tmp.path().join("code/vector_index.rs").exists());
    assert!(
        tmp.path()
            .join("product_design/product_roadmap_2025_vision.pptx")
            .exists()
    );
    assert!(tmp.path().join("engineering/architecture.txt").exists());
}

#[test]
fn test_query_suite_loading() {
    let suite = QuerySuite::load_auto(".").unwrap();
    assert!(
        suite.queries.len() >= 50,
        "Suite should contain at least 50 queries"
    );

    let exact_count = suite
        .queries
        .iter()
        .filter(|q| q.query_type == "exact-name")
        .count();
    let keyword_count = suite
        .queries
        .iter()
        .filter(|q| q.query_type == "keyword")
        .count();
    let semantic_count = suite
        .queries
        .iter()
        .filter(|q| q.query_type == "semantic")
        .count();
    let mixed_count = suite
        .queries
        .iter()
        .filter(|q| q.query_type == "mixed")
        .count();

    assert!(exact_count >= 10);
    assert!(keyword_count >= 10);
    assert!(semantic_count >= 10);
    assert!(mixed_count >= 10);
}

#[test]
fn test_regression_gate_logic() {
    let report = EvalReport {
        total_queries: 10,
        mrr: 0.82,
        recall_at_1: 0.70,
        recall_at_5: 0.85,
        recall_at_10: 0.90,
        avg_latency_ms: 12.0,
        by_category: std::collections::HashMap::new(),
        details: Vec::new(),
    };

    // Baseline: 0.85, max drop: 0.05 -> floor 0.80 -> 0.82 should pass
    assert!(check_regression(&report, 0.85, 0.05).is_ok());

    // Baseline: 0.90, max drop: 0.05 -> floor 0.85 -> 0.82 should fail
    assert!(check_regression(&report, 0.90, 0.05).is_err());
}
