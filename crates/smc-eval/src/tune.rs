use crate::evaluator::evaluate_suite;
use crate::queries::QuerySuite;
use smc_core::db::Database;
use smc_core::error::CoreResult;
use smc_embed::embedder::Embedder;
use smc_embed::vector_index::VectorIndex;
use smc_search::ranking::RankingConfig;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TuningResult {
    pub best_config: RankingConfig,
    pub best_mrr: f64,
    pub best_recall_at_10: f64,
    pub total_combinations: usize,
    pub elapsed_secs: f64,
}

/// Grid search hyperparameter optimizer for RankingConfig weights and signals.
pub fn grid_search_ranking_config(
    db: &Database,
    embedder: Option<&dyn Embedder>,
    vector_index: Option<&dyn VectorIndex>,
    suite: &QuerySuite,
) -> CoreResult<TuningResult> {
    let start_time = Instant::now();

    let rrf_k_candidates = [30.0, 60.0, 90.0];
    let weight_filename_candidates = [0.8, 1.2, 1.5];
    let weight_content_candidates = [0.8, 1.0, 1.2];
    let weight_vector_candidates = [0.8, 1.1, 1.4];
    let exact_boost_candidates = [1.5, 2.0, 2.5];
    let multi_chunk_candidates = [0.02, 0.05];

    let mut best_config = RankingConfig::default();
    let mut best_mrr = -1.0;
    let mut best_recall = -1.0;
    let mut total_trials = 0;

    println!("\n=== STARTING GRID-SEARCH HYPERPARAMETER TUNING ===");
    println!(
        "Evaluating candidate configurations against {} queries...\n",
        suite.queries.len()
    );

    for &k in &rrf_k_candidates {
        for &w_fn in &weight_filename_candidates {
            for &w_cnt in &weight_content_candidates {
                for &w_vec in &weight_vector_candidates {
                    for &exact_b in &exact_boost_candidates {
                        for &multi_b in &multi_chunk_candidates {
                            total_trials += 1;

                            let cfg = RankingConfig {
                                rrf_k: k,
                                weight_filename: w_fn,
                                weight_content: w_cnt,
                                weight_vector: w_vec,
                                exact_name_boost: exact_b,
                                multi_chunk_bonus: multi_b,
                                ..Default::default()
                            };

                            let report = evaluate_suite(db, embedder, vector_index, suite, &cfg)?;

                            if report.mrr > best_mrr
                                || (report.mrr == best_mrr && report.recall_at_10 > best_recall)
                            {
                                best_mrr = report.mrr;
                                best_recall = report.recall_at_10;
                                best_config = cfg.clone();

                                println!(
                                    "Trial #{:03}: NEW BEST -> MRR: {:.4} | Recall@10: {:.2}% | k: {:.1}, w_fn: {:.1}, w_cnt: {:.1}, w_vec: {:.1}, exact: {:.1}",
                                    total_trials,
                                    best_mrr,
                                    best_recall * 100.0,
                                    cfg.rrf_k,
                                    cfg.weight_filename,
                                    cfg.weight_content,
                                    cfg.weight_vector,
                                    cfg.exact_name_boost
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let elapsed = start_time.elapsed().as_secs_f64();
    println!("\n==================================================");
    println!("               GRID SEARCH COMPLETE               ");
    println!("==================================================");
    println!("Total Trials:       {}", total_trials);
    println!("Duration:           {:.2} seconds", elapsed);
    println!("Optimal MRR:        {:.4}", best_mrr);
    println!("Optimal Recall@10:  {:.2}%", best_recall * 100.0);
    println!("Optimal Config:     {:#?}", best_config);
    println!("==================================================\n");

    Ok(TuningResult {
        best_config,
        best_mrr,
        best_recall_at_10: best_recall,
        total_combinations: total_trials,
        elapsed_secs: elapsed,
    })
}
