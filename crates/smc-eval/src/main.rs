use smc_core::db::Database;
use smc_embed::embedder::{EmbeddingModelConfig, OnnxEmbedder};
use smc_embed::vector_index::SqliteVectorIndex;
use smc_eval::corpus::generate_synthetic_corpus;
use smc_eval::evaluator::{check_regression, evaluate_suite, index_corpus_directory};
use smc_eval::queries::QuerySuite;
use smc_eval::tune::grid_search_ranking_config;
use smc_search::ranking::RankingConfig;
use std::env;
use std::path::PathBuf;
use tempfile::tempdir;

fn print_usage() {
    println!("Usage: smc-eval <command> [options]");
    println!();
    println!("Commands:");
    println!("  eval     Run the evaluation suite (default) and check regression gate");
    println!("  tune     Run grid-search parameter tuning to optimize RankingConfig");
    println!("  generate Generate synthetic benchmark corpus files into target directory");
    println!();
    println!("Options:");
    println!("  --corpus <dir>        Directory with evaluation files (default: auto-generated)");
    println!(
        "  --queries <file>      Path to queries.toml (default: queries.local.toml or embedded)"
    );
    println!("  --models-dir <dir>    Path to models directory (default: ./models)");
    println!("  --baseline-mrr <num>  Baseline MRR threshold for regression gate (default: 0.85)");
    println!("  --max-drop <num>      Allowed MRR drop tolerance (default: 0.05)");
    println!(
        "  --no-embeddings       Disable ONNX vector embeddings (test keyword + filename only)"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("eval");

    if command == "--help" || command == "-h" {
        print_usage();
        return Ok(());
    }

    let mut corpus_arg: Option<PathBuf> = None;
    let mut queries_arg: Option<PathBuf> = None;
    let mut models_dir = PathBuf::from("models");
    let mut baseline_mrr = 0.85;
    let mut max_drop = 0.05;
    let mut disable_embeddings = false;
    let mut verbose = false;

    let mut idx = 2;
    while idx < args.len() {
        match args[idx].as_str() {
            "--verbose" | "-v" => {
                verbose = true;
                idx += 1;
            }
            "--corpus" => {
                if idx + 1 < args.len() {
                    corpus_arg = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            "--queries" => {
                if idx + 1 < args.len() {
                    queries_arg = Some(PathBuf::from(&args[idx + 1]));
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            "--models-dir" => {
                if idx + 1 < args.len() {
                    models_dir = PathBuf::from(&args[idx + 1]);
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            "--baseline-mrr" => {
                if idx + 1 < args.len() {
                    baseline_mrr = args[idx + 1].parse().unwrap_or(0.85);
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            "--max-drop" => {
                if idx + 1 < args.len() {
                    max_drop = args[idx + 1].parse().unwrap_or(0.05);
                    idx += 2;
                } else {
                    idx += 1;
                }
            }
            "--no-embeddings" => {
                disable_embeddings = true;
                idx += 1;
            }
            _ => {
                idx += 1;
            }
        }
    }

    if command == "generate" {
        let target = corpus_arg.unwrap_or_else(|| PathBuf::from("target/eval_corpus"));
        println!("Generating synthetic corpus at: {}", target.display());
        let count = generate_synthetic_corpus(&target)?;
        println!("Successfully generated {} benchmark files.", count);
        return Ok(());
    }

    // Prepare tempdir for evaluation if corpus is not provided
    let tmp_dir = tempdir()?;
    let corpus_path = if let Some(custom) = corpus_arg {
        custom
    } else {
        println!("Generating synthetic test corpus in temporary directory...");
        let path = tmp_dir.path().join("corpus");
        let count = generate_synthetic_corpus(&path)?;
        println!("Generated {} synthetic files across 8 categories.", count);
        path
    };

    // Load query suite
    let suite = if let Some(q_path) = queries_arg {
        println!("Loading queries from: {}", q_path.display());
        QuerySuite::from_file(&q_path)?
    } else {
        QuerySuite::load_auto(".")?
    };
    println!("Loaded query suite with {} queries.", suite.queries.len());

    // Setup SQLite DB in tempdir
    let db_path = tmp_dir.path().join("eval_index.db");
    let db = Database::open(&db_path)?;

    // Setup embedder if available
    let (embedder, vector_index) =
        if !disable_embeddings && models_dir.join("bge-small-en-v1.5").exists() {
            println!(
                "Loading ONNX embedding model from: {}",
                models_dir.display()
            );
            let model_cfg = EmbeddingModelConfig::default_bge_small(&models_dir);
            let emb = OnnxEmbedder::new(model_cfg);
            let vec_idx = SqliteVectorIndex::new(db.clone());
            (Some(emb), Some(vec_idx))
        } else {
            println!("Running without ONNX vector model (keyword and filename search only).");
            (None, None)
        };

    let emb_ref = embedder
        .as_ref()
        .map(|e| e as &dyn smc_embed::embedder::Embedder);
    let vec_ref = vector_index
        .as_ref()
        .map(|v| v as &dyn smc_embed::vector_index::VectorIndex);

    // Index the corpus
    println!("Indexing corpus...");
    let indexed_files = index_corpus_directory(&corpus_path, &db, emb_ref, vec_ref)?;
    println!("Indexed {} files successfully.\n", indexed_files);

    match command {
        "tune" => {
            println!("Executing hyperparameter grid search...");
            let tuning_res = grid_search_ranking_config(&db, emb_ref, vec_ref, &suite)?;
            println!("Tuning completed. Best MRR: {:.4}", tuning_res.best_mrr);
        }
        _ => {
            let config = RankingConfig::default();
            println!("Evaluating query suite with standard RankingConfig...");
            let report = evaluate_suite(&db, emb_ref, vec_ref, &suite, &config)?;
            report.print_summary_table();

            if verbose {
                println!("\nDetailed Query Results:");
                for d in &report.details {
                    let rank_str = match d.rank {
                        Some(r) => format!("Rank #{}", r),
                        None => "MISS (Rank >10)".to_string(),
                    };
                    println!(
                        "[{:<8}] {:<12} | {:<20} | Query: {:<50} | Expected: {:?}",
                        d.query_type, d.id, rank_str, d.query, d.expected_files
                    );
                    if d.rank.is_none() || d.rank.unwrap_or(0) > 1 {
                        println!("     Top matches: {:?}", d.top_matches);
                    }
                }
                println!();
            }

            // Check regression gate
            println!(
                "Checking regression gate (baseline: {:.4}, max drop: {:.4})...",
                baseline_mrr, max_drop
            );
            check_regression(&report, baseline_mrr, max_drop)?;
            println!("All evaluation checks PASSED!");
        }
    }

    Ok(())
}
