use crate::hybrid::SearchResult;
use rusqlite::OptionalExtension;
use smc_core::db::Database;
use smc_core::error::CoreResult;
use smc_embed::embedder::Embedder;
use smc_embed::vector_index::VectorIndex;
use std::collections::HashMap;

type ChunkRow = (String, Option<i64>, Option<String>, Option<String>);
type FileRow = (String, String, String, String, i64, String, String, String);

/// Details of a matching chunk aggregated during semantic search.
#[derive(Debug, Clone)]
struct ChunkHit {
    #[allow(dead_code)]
    chunk_id: i64,
    score: f32,
    text: String,
    page: Option<usize>,
    section: Option<String>,
    symbol: Option<String>,
}

/// Executes a semantic vector search across chunk embeddings and aggregates matches to files.
///
/// 1. Embeds the user query into an L2-normalized vector using `embedder`.
/// 2. Searches `vector_index` for top matching chunks for `embedder.model_id()`.
/// 3. Aggregates chunk hits by `file_id`:
///    - Base file score is the maximum chunk similarity score.
///    - Bonus multiplier for files with multiple matching chunks: `bonus = 0.03 * (matching_chunks - 1).min(3)`.
/// 4. Loads file and chunk metadata from SQLite.
/// 5. Ranks files descending by score and returns top `limit` results.
pub fn semantic_search(
    db: &Database,
    embedder: &dyn Embedder,
    vector_index: &dyn VectorIndex,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<SearchResult>> {
    let query = query.trim();
    if query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    // 1. Generate query embedding
    let query_vec = match embedder.embed_query(query) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "failed to embed search query");
            return Ok(Vec::new());
        }
    };

    // 2. Retrieve top matching chunks
    let chunk_limit = (limit * 4).max(20);
    let matches = vector_index
        .search(&query_vec, chunk_limit, Some(embedder.model_id()))
        .map_err(|e| {
            smc_core::error::CoreError::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })?;

    if matches.is_empty() {
        return Ok(Vec::new());
    }

    let reader = db.reader()?;

    // 3. Collect chunk details from SQLite
    let mut file_chunks: HashMap<i64, Vec<ChunkHit>> = HashMap::new();

    for m in matches {
        let chunk_row: Option<ChunkRow> = reader
            .query_row(
                "SELECT text, page, section, symbol FROM chunks WHERE id = ?1",
                rusqlite::params![m.chunk_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        if let Some((text, page, section, symbol)) = chunk_row {
            file_chunks.entry(m.file_id).or_default().push(ChunkHit {
                chunk_id: m.chunk_id,
                score: m.score,
                text,
                page: page.map(|p| p as usize),
                section,
                symbol,
            });
        }
    }

    // 4. Fetch file metadata and aggregate scores
    let mut results = Vec::new();

    for (file_id, mut chunks) in file_chunks {
        chunks.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let best_chunk = &chunks[0];
        let max_score = best_chunk.score as f64;

        // Multi-chunk bonus (diminishing return up to +0.09)
        let multi_chunk_bonus = 0.03 * (chunks.len() as f64 - 1.0).clamp(0.0, 3.0);
        let final_score = max_score + multi_chunk_bonus;

        // Fetch file record
        let file_row: Option<FileRow> = reader
            .query_row(
                "SELECT path, parent_dir, name, ext, size, mtime, kind, status FROM files WHERE id = ?1",
                rusqlite::params![file_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;

        if let Some((path, parent_dir, name, ext, size, mtime, kind, status)) = file_row {
            if status == "deleted" {
                continue;
            }

            // Create snippet
            let snippet = if best_chunk.text.len() <= 160 {
                best_chunk.text.clone()
            } else {
                format!("{}...", best_chunk.text[..160].trim_end())
            };

            results.push(SearchResult {
                id: file_id,
                path,
                name,
                parent_dir,
                ext,
                size,
                mtime,
                kind,
                score: final_score,
                match_type: "semantic".to_string(),
                snippet: Some(snippet),
                page: best_chunk.page,
                section: best_chunk.section.clone(),
                symbol: best_chunk.symbol.clone(),
                matches: Vec::new(),
                tag: None,
                masked_payload: None,
                thumbnail_path: None,
                is_screenshot: false,
            });
        }
    }

    // 5. Sort descending by score
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if results.len() > limit {
        results.truncate(limit);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smc_core::db::Database;
    use smc_embed::error::EmbedResult;
    use smc_embed::vector_index::{SqliteVectorIndex, VectorRecord};

    struct TestMockEmbedder {
        model_id: String,
        dim: usize,
    }

    impl TestMockEmbedder {
        fn new(model_id: &str, dim: usize) -> Self {
            Self {
                model_id: model_id.to_string(),
                dim,
            }
        }
    }

    impl Embedder for TestMockEmbedder {
        fn embed_documents(&self, docs: &[&str]) -> EmbedResult<Vec<Vec<f32>>> {
            let mut res = Vec::new();
            for doc in docs {
                let mut v = vec![0.0f32; self.dim];
                for (i, b) in doc.bytes().enumerate() {
                    v[i % self.dim] += b as f32;
                }
                let sum_sq: f32 = v.iter().map(|x| x * x).sum();
                let norm = sum_sq.sqrt().max(1e-12);
                for x in &mut v {
                    *x /= norm;
                }
                res.push(v);
            }
            Ok(res)
        }

        fn embed_query(&self, query: &str) -> EmbedResult<Vec<f32>> {
            let embs = self.embed_documents(&[query])?;
            Ok(embs[0].clone())
        }

        fn dims(&self) -> usize {
            self.dim
        }

        fn model_id(&self) -> &str {
            &self.model_id
        }

        fn unload(&self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn maybe_unload_idle(&self) -> bool {
            false
        }
    }

    #[test]
    fn test_semantic_search_aggregation_and_multi_chunk_bonus() {
        let db = Database::open_in_memory().unwrap();
        let vector_index = SqliteVectorIndex::new(db.clone());
        let embedder = TestMockEmbedder::new("test-embedder", 32);

        // Seed DB with 2 files and multiple chunks
        {
            let conn = db.writer();
            conn.execute_batch(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (1, '/docs/ai.md', '/docs', 'ai.md', 'md', 500, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
                 INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (2, '/docs/cooking.md', '/docs', 'cooking.md', 'md', 500, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
                 INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                 VALUES (101, 1, 0, 'Machine learning and deep neural networks in artificial intelligence.', 1, 'Intro', NULL, 0, 70);
                 INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                 VALUES (102, 1, 1, 'Transformers and attention mechanisms in large language models.', 2, 'Models', NULL, 71, 140);
                 INSERT INTO chunks (id, file_id, ordinal, text, page, section, symbol, start, end)
                 VALUES (103, 2, 0, 'Delicious chocolate cake recipe with flour, eggs, and cocoa powder.', 1, 'Recipe', NULL, 0, 70);"
            ).unwrap();
        }

        // Insert vector records
        let c101_vec = embedder
            .embed_query("Machine learning and deep neural networks in artificial intelligence.")
            .unwrap();
        let c102_vec = embedder
            .embed_query("Transformers and attention mechanisms in large language models.")
            .unwrap();
        let c103_vec = embedder
            .embed_query("Delicious chocolate cake recipe with flour, eggs, and cocoa powder.")
            .unwrap();

        vector_index
            .insert_batch(&[
                VectorRecord {
                    chunk_id: 101,
                    file_id: 1,
                    model_id: "test-embedder".into(),
                    text_hash: "h1".into(),
                    vector: c101_vec,
                },
                VectorRecord {
                    chunk_id: 102,
                    file_id: 1,
                    model_id: "test-embedder".into(),
                    text_hash: "h2".into(),
                    vector: c102_vec,
                },
                VectorRecord {
                    chunk_id: 103,
                    file_id: 2,
                    model_id: "test-embedder".into(),
                    text_hash: "h3".into(),
                    vector: c103_vec,
                },
            ])
            .unwrap();

        // Search for AI query
        let results = semantic_search(
            &db,
            &embedder,
            &vector_index,
            "neural networks and machine learning",
            5,
        )
        .unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].id, 1);
        assert_eq!(results[0].name, "ai.md");
        assert_eq!(results[0].match_type, "semantic");
        assert!(results[0].snippet.is_some());
    }
}
