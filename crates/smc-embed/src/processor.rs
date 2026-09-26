use crate::embedder::Embedder;
use crate::error::EmbedResult;
use crate::hash::hash_chunk_text;
use crate::vector_index::{VectorIndex, VectorRecord};
use smc_core::db::Database;
use tracing::{debug, info};

/// Result summary of processing an embedding job for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedJobResult {
    pub file_id: i64,
    pub total_chunks: usize,
    pub embedded_chunks: usize,
    pub skipped_chunks: usize,
    pub pruned_vectors: usize,
}

/// Processes chunk embeddings for a specific file.
///
/// 1. Reads all current chunks for `file_id`.
/// 2. Retrieves existing indexed chunk hashes for `embedder.model_id()`.
/// 3. Skips chunks whose hash matches what is already in the vector index.
/// 4. Generates embeddings in batch for new/modified chunks and inserts them.
/// 5. Prunes vector records for chunk IDs that no longer exist for this file.
pub fn process_file_embedding(
    db: &Database,
    file_id: i64,
    embedder: &dyn Embedder,
    vector_index: &dyn VectorIndex,
) -> EmbedResult<EmbedJobResult> {
    let reader = db.reader()?;
    let model_id = embedder.model_id();

    // 1. Fetch current chunks for this file from SQLite
    let mut stmt =
        reader.prepare("SELECT id, text FROM chunks WHERE file_id = ?1 ORDER BY ordinal ASC")?;
    let chunk_rows = stmt.query_map(rusqlite::params![file_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut current_chunks = Vec::new();
    for r in chunk_rows {
        current_chunks.push(r?);
    }

    if current_chunks.is_empty() {
        // If file has no chunks (e.g., deleted or non-text), clean up any lingering vectors
        let deleted = vector_index.delete_for_file(file_id)?;
        return Ok(EmbedJobResult {
            file_id,
            total_chunks: 0,
            embedded_chunks: 0,
            skipped_chunks: 0,
            pruned_vectors: deleted,
        });
    }

    // 2. Fetch existing chunk hashes for this file and model
    let existing_hashes = vector_index.get_indexed_chunk_hashes(file_id, model_id)?;

    // 3. Identify chunks to embed and chunks to skip
    let mut chunks_to_embed = Vec::new();
    let mut skipped_count = 0;
    let mut current_chunk_ids = std::collections::HashSet::new();

    for (chunk_id, text) in &current_chunks {
        current_chunk_ids.insert(*chunk_id);
        let hash = hash_chunk_text(text);

        if let Some(existing_hash) = existing_hashes.get(chunk_id)
            && existing_hash == &hash
        {
            // Chunk text unchanged; skip embedding
            skipped_count += 1;
            continue;
        }

        chunks_to_embed.push((*chunk_id, hash, text.as_str()));
    }

    // 4. Prune vectors for chunks that no longer exist
    let mut pruned_count = 0;
    for &old_chunk_id in existing_hashes.keys() {
        if !current_chunk_ids.contains(&old_chunk_id) {
            pruned_count += vector_index.delete_for_chunk(old_chunk_id)?;
        }
    }

    // 5. Batch embed changed/new chunks
    let embedded_count = chunks_to_embed.len();
    if !chunks_to_embed.is_empty() {
        let texts: Vec<&str> = chunks_to_embed.iter().map(|(_, _, t)| *t).collect();
        let vectors = embedder.embed_documents(&texts)?;

        let mut records = Vec::with_capacity(chunks_to_embed.len());
        for (i, &(chunk_id, ref hash, _)) in chunks_to_embed.iter().enumerate() {
            records.push(VectorRecord {
                chunk_id,
                file_id,
                model_id: model_id.to_string(),
                text_hash: hash.clone(),
                vector: vectors[i].clone(),
            });
        }

        vector_index.insert_batch(&records)?;
    }

    debug!(
        file_id,
        total = current_chunks.len(),
        embedded = embedded_count,
        skipped = skipped_count,
        pruned = pruned_count,
        "processed file embeddings"
    );

    Ok(EmbedJobResult {
        file_id,
        total_chunks: current_chunks.len(),
        embedded_chunks: embedded_count,
        skipped_chunks: skipped_count,
        pruned_vectors: pruned_count,
    })
}

/// Enqueues embed jobs for all active files that have chunks without up-to-date vectors.
pub fn enqueue_missing_embeddings(db: &Database, model_id: &str) -> EmbedResult<usize> {
    let conn = db.writer();
    let now = chrono::Utc::now().to_rfc3339();

    let count = conn.execute(
        "INSERT OR IGNORE INTO jobs (kind, file_id, priority, state, created_at)
         SELECT DISTINCT 'embed', c.file_id, 3, 'pending', ?1
         FROM chunks c
         JOIN files f ON f.id = c.file_id
         LEFT JOIN chunk_vectors cv ON cv.chunk_id = c.id AND cv.model_id = ?2
         WHERE f.status = 'active' AND cv.chunk_id IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM jobs WHERE file_id = c.file_id AND kind = 'embed' AND state IN ('pending', 'running')
           )",
        rusqlite::params![now, model_id],
    )?;

    if count > 0 {
        info!(
            enqueued_jobs = count,
            model_id, "enqueued missing embedding jobs"
        );
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector_index::SqliteVectorIndex;
    use smc_core::db::Database;

    /// Mock embedder for testing processor logic without requiring ONNX models.
    struct MockEmbedder {
        model_id: String,
        dim: usize,
    }

    impl MockEmbedder {
        fn new(model_id: &str, dim: usize) -> Self {
            Self {
                model_id: model_id.to_string(),
                dim,
            }
        }
    }

    impl Embedder for MockEmbedder {
        fn embed_documents(&self, docs: &[&str]) -> EmbedResult<Vec<Vec<f32>>> {
            let mut results = Vec::new();
            for doc in docs {
                let mut v = vec![0.0f32; self.dim];
                for (i, byte) in doc.bytes().enumerate() {
                    v[i % self.dim] += byte as f32;
                }
                // Normalize
                let sum_sq: f32 = v.iter().map(|x| x * x).sum();
                let norm = sum_sq.sqrt().max(1e-12);
                for x in &mut v {
                    *x /= norm;
                }
                results.push(v);
            }
            Ok(results)
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
    fn test_processor_embedding_dedup_and_model_change() {
        let db = Database::open_in_memory().unwrap();
        let index = SqliteVectorIndex::new(db.clone());

        // Setup test database
        {
            let conn = db.writer();
            conn.execute_batch(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (1, '/docs/test.md', '/docs', 'test.md', 'md', 500, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
                 INSERT INTO chunks (id, file_id, ordinal, text, start, end)
                 VALUES (101, 1, 0, 'Chunk one content', 0, 17);
                 INSERT INTO chunks (id, file_id, ordinal, text, start, end)
                 VALUES (102, 1, 1, 'Chunk two content', 18, 35);"
            ).unwrap();
        }

        let embedder_v1 = MockEmbedder::new("model-v1", 64);

        // 1. Initial embedding: should embed both chunks
        let res1 = process_file_embedding(&db, 1, &embedder_v1, &index).unwrap();
        assert_eq!(res1.total_chunks, 2);
        assert_eq!(res1.embedded_chunks, 2);
        assert_eq!(res1.skipped_chunks, 0);
        assert_eq!(index.count(Some("model-v1")).unwrap(), 2);

        // 2. Second pass with no changes: should skip both chunks
        let res2 = process_file_embedding(&db, 1, &embedder_v1, &index).unwrap();
        assert_eq!(res2.embedded_chunks, 0);
        assert_eq!(res2.skipped_chunks, 2);

        // 3. Update chunk 102 text in DB
        {
            let conn = db.writer();
            conn.execute(
                "UPDATE chunks SET text = 'Chunk two modified text' WHERE id = 102",
                [],
            )
            .unwrap();
        }

        // 4. Third pass: chunk 101 skipped, chunk 102 re-embedded
        let res3 = process_file_embedding(&db, 1, &embedder_v1, &index).unwrap();
        assert_eq!(res3.embedded_chunks, 1);
        assert_eq!(res3.skipped_chunks, 1);

        // 5. Switch to a new model_id: should embed both chunks under model-v2
        let embedder_v2 = MockEmbedder::new("model-v2", 64);
        let res4 = process_file_embedding(&db, 1, &embedder_v2, &index).unwrap();
        assert_eq!(res4.embedded_chunks, 2);
        assert_eq!(res4.skipped_chunks, 0);
        assert_eq!(index.count(Some("model-v1")).unwrap(), 0);
        assert_eq!(index.count(Some("model-v2")).unwrap(), 2);
    }
}
