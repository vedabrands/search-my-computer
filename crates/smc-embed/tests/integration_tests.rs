use smc_core::db::Database;
use smc_embed::embedder::{DEFAULT_MODEL_ID, Embedder, EmbeddingModelConfig, OnnxEmbedder};
use smc_embed::vector_index::{SqliteVectorIndex, VectorIndex, VectorRecord};
use std::path::PathBuf;

fn find_models_dir() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("../../models"),
        PathBuf::from("../models"),
        PathBuf::from("models"),
    ];

    for c in &candidates {
        if c.exists() && c.join(DEFAULT_MODEL_ID).exists() {
            return Some(c.clone());
        }
    }
    None
}

#[test]
fn test_onnx_embedder_bge_small_inference() {
    let Some(models_dir) = find_models_dir() else {
        println!("Skipping ONNX test: models directory not found");
        return;
    };

    let config = EmbeddingModelConfig::default_bge_small(&models_dir);
    let embedder = OnnxEmbedder::new(config);

    assert!(!embedder.is_loaded());

    // 1. Embed query and document
    let query_vec = embedder.embed_query("machine learning algorithms").unwrap();
    assert!(embedder.is_loaded());
    assert_eq!(query_vec.len(), 384);

    let doc_texts = [
        "Deep learning and neural network architectures for classification",
        "A recipe for chocolate chip cookies with butter and sugar",
    ];
    let doc_vecs = embedder.embed_documents(&doc_texts).unwrap();
    assert_eq!(doc_vecs.len(), 2);
    assert_eq!(doc_vecs[0].len(), 384);
    assert_eq!(doc_vecs[1].len(), 384);

    // Compute dot products
    let sim_ml: f32 = query_vec.iter().zip(&doc_vecs[0]).map(|(a, b)| a * b).sum();
    let sim_recipe: f32 = query_vec.iter().zip(&doc_vecs[1]).map(|(a, b)| a * b).sum();

    println!("Cosine similarity ML query to ML doc: {sim_ml:.4}");
    println!("Cosine similarity ML query to Recipe doc: {sim_recipe:.4}");

    assert!(
        sim_ml > sim_recipe,
        "ML document ({sim_ml}) should rank higher than recipe ({sim_recipe})"
    );
    assert!(sim_ml > 0.60, "Expected high similarity for related text");
    assert!(
        sim_recipe < 0.40,
        "Expected low similarity for unrelated text"
    );

    // 2. Test unload
    embedder.unload();
    assert!(!embedder.is_loaded());
}

#[test]
fn test_onnx_embedder_lite_minilm_inference() {
    let Some(models_dir) = find_models_dir() else {
        println!("Skipping ONNX test: models directory not found");
        return;
    };

    let config = EmbeddingModelConfig::lite_minilm(&models_dir);
    if !config.model_path.exists() {
        println!("Skipping MiniLM test: lite model not present");
        return;
    }

    let embedder = OnnxEmbedder::new(config);
    let q_vec = embedder.embed_query("quantum mechanics").unwrap();
    assert_eq!(q_vec.len(), 384);

    let docs = embedder
        .embed_documents(&[
            "Schrodinger equation and wave function collapse in physics",
            "How to bake artisan sourdough bread at home",
        ])
        .unwrap();

    let sim_physics: f32 = q_vec.iter().zip(&docs[0]).map(|(a, b)| a * b).sum();
    let sim_bread: f32 = q_vec.iter().zip(&docs[1]).map(|(a, b)| a * b).sum();

    println!("MiniLM sim_physics: {sim_physics:.4}, sim_bread: {sim_bread:.4}");

    assert!(sim_physics > sim_bread);
    assert!(sim_physics > 0.45);
}

#[test]
fn test_end_to_end_embedding_and_sqlite_vector_search() {
    let Some(models_dir) = find_models_dir() else {
        println!("Skipping ONNX test: models directory not found");
        return;
    };

    let db = Database::open_in_memory().unwrap();
    let index = SqliteVectorIndex::new(db.clone());
    let config = EmbeddingModelConfig::default_bge_small(&models_dir);
    let embedder = OnnxEmbedder::new(config);

    // Setup database files and chunks
    {
        let conn = db.writer();
        conn.execute_batch(
            "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
             VALUES (1, '/docs/rust.md', '/docs', 'rust.md', 'md', 500, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
             INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
             VALUES (2, '/docs/cooking.md', '/docs', 'cooking.md', 'md', 500, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
             INSERT INTO chunks (id, file_id, ordinal, text, start, end)
             VALUES (101, 1, 0, 'Rust memory safety without garbage collection and ownership model.', 0, 60);
             INSERT INTO chunks (id, file_id, ordinal, text, start, end)
             VALUES (102, 2, 0, 'Italian pasta carbonara with eggs, pecorino cheese, and guanciale.', 0, 60);"
        ).unwrap();
    }

    let chunks = [
        (
            101i64,
            1i64,
            "Rust memory safety without garbage collection and ownership model.",
        ),
        (
            102i64,
            2i64,
            "Italian pasta carbonara with eggs, pecorino cheese, and guanciale.",
        ),
    ];

    let texts: Vec<&str> = chunks.iter().map(|(_, _, t)| *t).collect();
    let embs = embedder.embed_documents(&texts).unwrap();

    let mut records = Vec::new();
    for (i, &(chunk_id, file_id, text)) in chunks.iter().enumerate() {
        records.push(VectorRecord {
            chunk_id,
            file_id,
            model_id: embedder.model_id().to_string(),
            text_hash: smc_embed::hash_chunk_text(text),
            vector: embs[i].clone(),
        });
    }

    index.insert_batch(&records).unwrap();
    assert_eq!(index.count(None).unwrap(), 2);

    // Search for borrow checker / memory management query
    let q_vec = embedder.embed_query("ownership and memory safety").unwrap();
    let matches = index.search(&q_vec, 2, Some(embedder.model_id())).unwrap();

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].chunk_id, 101);
    assert_eq!(matches[0].file_id, 1);
    assert!(matches[0].score > matches[1].score);
    assert!(matches[0].score > 0.65);
}

#[test]
fn test_vector_index_recall_and_quantization_precision() {
    let db = Database::open_in_memory().unwrap();
    let index = SqliteVectorIndex::new(db.clone());
    let dim = 384;
    let count = 500;

    // Seed database files and chunks
    {
        let conn = db.writer();
        let tx = conn.unchecked_transaction().unwrap();
        for i in 1..=count {
            tx.execute(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (?1, ?2, '/test', ?3, 'txt', 100, datetime('now'), datetime('now'), 'text', 'active', datetime('now'))",
                rusqlite::params![i, format!("/test/f{i}.txt"), format!("f{i}.txt")],
            ).unwrap();
            tx.execute(
                "INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                 VALUES (?1, ?1, 0, 'test chunk text', 1, 'Main', NULL, 0, 15)",
                rusqlite::params![i],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    // Generate deterministic synthetic unit vectors
    let mut raw_vectors = Vec::with_capacity(count);
    let mut records = Vec::with_capacity(count);

    for i in 1..=count {
        let mut v = Vec::with_capacity(dim);
        let mut seed = (i * 7919) as f64;
        for _ in 0..dim {
            seed = (seed * 9301.0 + 49297.0).rem_euclid(233280.0);
            let val = (seed / 233280.0) * 2.0 - 1.0;
            v.push(val as f32);
        }
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        for x in &mut v {
            *x /= norm;
        }

        records.push(VectorRecord {
            chunk_id: i as i64,
            file_id: i as i64,
            model_id: "test-model".into(),
            text_hash: format!("hash_{i}"),
            vector: v.clone(),
        });
        raw_vectors.push((i as i64, v));
    }

    index.insert_batch(&records).unwrap();

    // Query with 5 random query vectors
    for q_idx in 0..5 {
        let mut q_vec = Vec::with_capacity(dim);
        let mut seed = (q_idx * 1337 + 42) as f64;
        for _ in 0..dim {
            seed = (seed * 9301.0 + 49297.0).rem_euclid(233280.0);
            let val = (seed / 233280.0) * 2.0 - 1.0;
            q_vec.push(val as f32);
        }
        let norm: f32 = q_vec.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
        for x in &mut q_vec {
            *x /= norm;
        }

        // 1. Exact f32 in-memory brute force
        let mut exact_matches: Vec<(i64, f32)> = raw_vectors
            .iter()
            .map(|(id, v)| {
                let sim: f32 = q_vec.iter().zip(v).map(|(a, b)| a * b).sum();
                (*id, sim)
            })
            .collect();
        exact_matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_exact = &exact_matches[..10];

        // 2. SqliteVectorIndex f16 search
        let index_matches = index.search(&q_vec, 10, Some("test-model")).unwrap();
        assert_eq!(index_matches.len(), 10);

        // Verify top result matches and recall@10 is >= 90% (due to f16 float boundaries)
        assert_eq!(
            index_matches[0].chunk_id, top_exact[0].0,
            "Top 1 match must be identical between f16 and f32 search"
        );

        let exact_ids: std::collections::HashSet<i64> =
            top_exact.iter().map(|(id, _)| *id).collect();
        let index_ids: std::collections::HashSet<i64> =
            index_matches.iter().map(|m| m.chunk_id).collect();
        let overlap = exact_ids.intersection(&index_ids).count();
        assert!(
            overlap >= 9,
            "Recall@10 for f16 vector index was {overlap}/10 (expected >= 9)"
        );

        // Check cosine score difference is within 0.001
        for (m, exact) in index_matches.iter().zip(top_exact.iter()) {
            if m.chunk_id == exact.0 {
                let diff = (m.score - exact.1).abs();
                assert!(
                    diff < 0.002,
                    "f16 quantization score difference ({diff}) exceeded tolerance"
                );
            }
        }
    }
}

#[test]
fn test_model_migration_and_reembed_lifecycle() {
    let db = Database::open_in_memory().unwrap();
    let index = SqliteVectorIndex::new(db.clone());

    // 1. Seed file and 2 chunks
    {
        let conn = db.writer();
        conn.execute_batch(
            "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
             VALUES (1, '/docs/system.md', '/docs', 'system.md', 'md', 300, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
             INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
             VALUES (1, 1, 0, 'Operating system process scheduling algorithms.', 1, 'OS', NULL, 0, 50);
             INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
             VALUES (2, 1, 1, 'Virtual memory paging and TLB translation.', 2, 'Memory', NULL, 51, 100);"
        ).unwrap();
    }

    struct MockTestEmbedder {
        id: &'static str,
    }
    impl Embedder for MockTestEmbedder {
        fn embed_documents(&self, docs: &[&str]) -> smc_embed::error::EmbedResult<Vec<Vec<f32>>> {
            Ok(docs
                .iter()
                .map(|d| vec![d.len() as f32 / 100.0; 384])
                .collect())
        }
        fn embed_query(&self, q: &str) -> smc_embed::error::EmbedResult<Vec<f32>> {
            Ok(vec![q.len() as f32 / 100.0; 384])
        }
        fn dims(&self) -> usize {
            384
        }
        fn model_id(&self) -> &str {
            self.id
        }
        fn unload(&self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn maybe_unload_idle(&self) -> bool {
            false
        }
    }

    let embedder_v1 = MockTestEmbedder { id: "model-v1" };
    let embedder_v2 = MockTestEmbedder { id: "model-v2" };

    // Embed with model v1
    let res1 = smc_embed::process_file_embedding(&db, 1, &embedder_v1, &index).unwrap();
    assert_eq!(res1.embedded_chunks, 2);
    assert_eq!(res1.skipped_chunks, 0);
    assert_eq!(index.count(Some("model-v1")).unwrap(), 2);
    assert_eq!(index.count(Some("model-v2")).unwrap(), 0);

    // Re-run with model v1: should skip all unchanged chunks
    let res1_repeat = smc_embed::process_file_embedding(&db, 1, &embedder_v1, &index).unwrap();
    assert_eq!(res1_repeat.embedded_chunks, 0);
    assert_eq!(res1_repeat.skipped_chunks, 2);

    // Switch active model to model v2: should embed for model v2
    let res2 = smc_embed::process_file_embedding(&db, 1, &embedder_v2, &index).unwrap();
    assert_eq!(res2.embedded_chunks, 2);
    assert_eq!(index.count(Some("model-v2")).unwrap(), 2);

    // Update chunk 1 text in DB
    {
        let conn = db.writer();
        conn.execute(
            "UPDATE chunks SET text = 'Updated OS process scheduling text.' WHERE id = 1",
            [],
        )
        .unwrap();
    }

    // Re-process: chunk 1 should be re-embedded, chunk 2 should be skipped
    let res2_update = smc_embed::process_file_embedding(&db, 1, &embedder_v2, &index).unwrap();
    assert_eq!(res2_update.embedded_chunks, 1);
    assert_eq!(res2_update.skipped_chunks, 1);

    // Test enqueue_missing_embeddings
    let queued = smc_embed::enqueue_missing_embeddings(&db, "model-v3").unwrap();
    assert_eq!(
        queued, 1,
        "Should queue file 1 because model-v3 has no embeddings yet"
    );
}
