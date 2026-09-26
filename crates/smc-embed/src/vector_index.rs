use crate::error::EmbedResult;
use half::f16;
use smc_core::db::Database;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// A vector record to be inserted into the index.
#[derive(Debug, Clone)]
pub struct VectorRecord {
    pub chunk_id: i64,
    pub file_id: i64,
    pub model_id: String,
    pub text_hash: String,
    pub vector: Vec<f32>,
}

/// A search result match from vector similarity.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatch {
    pub chunk_id: i64,
    pub file_id: i64,
    pub score: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct ScoredItem {
    chunk_id: i64,
    file_id: i64,
    score: f32,
}

impl Eq for ScoredItem {}

impl PartialOrd for ScoredItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Reverse ordering so BinaryHeap acts as a min-heap for top-K
impl Ord for ScoredItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // Lower score has higher priority in min-heap
        other
            .score
            .partial_cmp(&self.score)
            .unwrap_or(Ordering::Equal)
    }
}

/// Trait abstracting vector storage and similarity search.
///
/// Implementations can be SQLite (brute-force over f16 BLOBs for <200k vectors)
/// or an external HNSW index (e.g., usearch) for larger collections.
pub trait VectorIndex: Send + Sync {
    /// Inserts or replaces a single vector record.
    fn insert(&self, record: &VectorRecord) -> EmbedResult<()>;

    /// Inserts or replaces a batch of vector records within a transaction.
    fn insert_batch(&self, records: &[VectorRecord]) -> EmbedResult<()>;

    /// Performs top-K vector similarity search.
    fn search(
        &self,
        query_vec: &[f32],
        limit: usize,
        model_id: Option<&str>,
    ) -> EmbedResult<Vec<VectorMatch>>;

    /// Deletes all vector records associated with a file.
    fn delete_for_file(&self, file_id: i64) -> EmbedResult<usize>;

    /// Deletes the vector record for a specific chunk.
    fn delete_for_chunk(&self, chunk_id: i64) -> EmbedResult<usize>;

    /// Returns a map of chunk_id -> text_hash for all indexed vectors of a file.
    fn get_indexed_chunk_hashes(
        &self,
        file_id: i64,
        model_id: &str,
    ) -> EmbedResult<HashMap<i64, String>>;

    /// Returns the number of vectors in the index, optionally filtered by model_id.
    fn count(&self, model_id: Option<&str>) -> EmbedResult<usize>;

    /// Clears all vectors from the index.
    fn clear(&self) -> EmbedResult<()>;

    /// Retrieves the f32 vector for a chunk if present.
    fn get_vector(&self, chunk_id: i64) -> EmbedResult<Option<Vec<f32>>>;
}

/// SQLite-based vector index storing f16 quantized vectors in `chunk_vectors` table.
#[derive(Clone)]
pub struct SqliteVectorIndex {
    db: Database,
}

impl SqliteVectorIndex {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Converts an f32 vector into a packed little-endian f16 byte slice (768 bytes for 384 dims).
    pub fn vector_to_f16_blob(vec: &[f32]) -> Vec<u8> {
        let mut blob = Vec::with_capacity(vec.len() * 2);
        for &val in vec {
            let h = f16::from_f32(val);
            blob.extend_from_slice(&h.to_le_bytes());
        }
        blob
    }

    /// Converts a packed little-endian f16 byte slice back into an f32 vector.
    pub fn f16_blob_to_vector(blob: &[u8]) -> Vec<f32> {
        let (chunks, _) = blob.as_chunks::<2>();
        let mut vec = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let h = f16::from_le_bytes(*chunk);
            vec.push(h.to_f32());
        }
        vec
    }

    /// Computes dot product between an L2-normalized f32 query vector and an f16 packed vector blob.
    /// Since both vectors are L2-normalized, the dot product equals the cosine similarity.
    pub fn dot_product_f32_f16(query: &[f32], blob: &[u8]) -> f32 {
        let dims = query.len();
        if blob.len() < dims * 2 {
            return 0.0;
        }

        let mut sum = 0.0f32;
        let (chunks, _) = blob.as_chunks::<2>();

        // Unrolled dot product for CPU cache and SIMD autovectorization
        let mut q_iter = query.iter();
        for chunk in chunks.iter().take(dims) {
            let q = *q_iter.next().unwrap_or(&0.0);
            let val = f16::from_le_bytes(*chunk).to_f32();
            sum += q * val;
        }

        sum
    }
}

impl VectorIndex for SqliteVectorIndex {
    fn insert(&self, record: &VectorRecord) -> EmbedResult<()> {
        let blob = Self::vector_to_f16_blob(&record.vector);
        let conn = self.db.writer();

        conn.execute(
            "INSERT INTO chunk_vectors (chunk_id, file_id, model_id, dims, text_hash, vector)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(chunk_id) DO UPDATE SET
                file_id = excluded.file_id,
                model_id = excluded.model_id,
                dims = excluded.dims,
                text_hash = excluded.text_hash,
                vector = excluded.vector",
            rusqlite::params![
                record.chunk_id,
                record.file_id,
                record.model_id,
                record.vector.len() as i64,
                record.text_hash,
                blob,
            ],
        )?;

        Ok(())
    }

    fn insert_batch(&self, records: &[VectorRecord]) -> EmbedResult<()> {
        if records.is_empty() {
            return Ok(());
        }

        let mut conn = self.db.writer();
        let tx = conn.transaction()?;

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO chunk_vectors (chunk_id, file_id, model_id, dims, text_hash, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(chunk_id) DO UPDATE SET
                    file_id = excluded.file_id,
                    model_id = excluded.model_id,
                    dims = excluded.dims,
                    text_hash = excluded.text_hash,
                    vector = excluded.vector",
            )?;

            for record in records {
                let blob = Self::vector_to_f16_blob(&record.vector);
                stmt.execute(rusqlite::params![
                    record.chunk_id,
                    record.file_id,
                    record.model_id,
                    record.vector.len() as i64,
                    record.text_hash,
                    blob,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn search(
        &self,
        query_vec: &[f32],
        limit: usize,
        model_id: Option<&str>,
    ) -> EmbedResult<Vec<VectorMatch>> {
        if limit == 0 || query_vec.is_empty() {
            return Ok(Vec::new());
        }

        let reader = self.db.reader()?;
        let sql = match model_id {
            Some(_) => "SELECT chunk_id, file_id, vector FROM chunk_vectors WHERE model_id = ?1",
            None => "SELECT chunk_id, file_id, vector FROM chunk_vectors",
        };

        let mut stmt = reader.prepare(sql)?;
        let mut rows = match model_id {
            Some(m) => stmt.query(rusqlite::params![m])?,
            None => stmt.query([])?,
        };

        // Min-heap to maintain top-K items efficiently
        let mut heap: BinaryHeap<ScoredItem> = BinaryHeap::with_capacity(limit + 1);

        while let Some(row) = rows.next()? {
            let chunk_id: i64 = row.get(0)?;
            let file_id: i64 = row.get(1)?;
            let blob_ref = row.get_ref(2)?;
            let blob = blob_ref.as_blob()?;

            let score = Self::dot_product_f32_f16(query_vec, blob);

            if heap.len() < limit {
                heap.push(ScoredItem {
                    chunk_id,
                    file_id,
                    score,
                });
            } else if let Some(min_item) = heap.peek()
                && score > min_item.score
            {
                heap.pop();
                heap.push(ScoredItem {
                    chunk_id,
                    file_id,
                    score,
                });
            }
        }

        // Extract and sort results descending by score
        let mut results: Vec<VectorMatch> = heap
            .into_iter()
            .map(|item| VectorMatch {
                chunk_id: item.chunk_id,
                file_id: item.file_id,
                score: item.score,
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
        Ok(results)
    }

    fn delete_for_file(&self, file_id: i64) -> EmbedResult<usize> {
        let conn = self.db.writer();
        let deleted = conn.execute(
            "DELETE FROM chunk_vectors WHERE file_id = ?1",
            rusqlite::params![file_id],
        )?;
        Ok(deleted)
    }

    fn delete_for_chunk(&self, chunk_id: i64) -> EmbedResult<usize> {
        let conn = self.db.writer();
        let deleted = conn.execute(
            "DELETE FROM chunk_vectors WHERE chunk_id = ?1",
            rusqlite::params![chunk_id],
        )?;
        Ok(deleted)
    }

    fn get_indexed_chunk_hashes(
        &self,
        file_id: i64,
        model_id: &str,
    ) -> EmbedResult<HashMap<i64, String>> {
        let reader = self.db.reader()?;
        let mut stmt = reader.prepare(
            "SELECT chunk_id, text_hash FROM chunk_vectors WHERE file_id = ?1 AND model_id = ?2",
        )?;

        let rows = stmt.query_map(rusqlite::params![file_id, model_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut map = HashMap::new();
        for row in rows {
            let (chunk_id, hash) = row?;
            map.insert(chunk_id, hash);
        }

        Ok(map)
    }

    fn count(&self, model_id: Option<&str>) -> EmbedResult<usize> {
        let reader = self.db.reader()?;
        let count: i64 = match model_id {
            Some(m) => reader.query_row(
                "SELECT COUNT(*) FROM chunk_vectors WHERE model_id = ?1",
                rusqlite::params![m],
                |r| r.get(0),
            )?,
            None => reader.query_row("SELECT COUNT(*) FROM chunk_vectors", [], |r| r.get(0))?,
        };
        Ok(count as usize)
    }

    fn clear(&self) -> EmbedResult<()> {
        let conn = self.db.writer();
        conn.execute("DELETE FROM chunk_vectors", [])?;
        Ok(())
    }

    fn get_vector(&self, chunk_id: i64) -> EmbedResult<Option<Vec<f32>>> {
        let reader = self.db.reader()?;
        let mut stmt = reader.prepare("SELECT vector FROM chunk_vectors WHERE chunk_id = ?1")?;

        let mut rows = stmt.query(rusqlite::params![chunk_id])?;
        if let Some(row) = rows.next()? {
            let blob_ref = row.get_ref(0)?;
            let blob = blob_ref.as_blob()?;
            Ok(Some(Self::f16_blob_to_vector(blob)))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_l2_normalized_vector(seed: f32, dims: usize) -> Vec<f32> {
        let mut vec = Vec::with_capacity(dims);
        let mut sum_sq = 0.0f32;
        for i in 0..dims {
            let val = (seed * (i as f32 + 1.0)).sin();
            vec.push(val);
            sum_sq += val * val;
        }
        let norm = sum_sq.sqrt();
        for v in &mut vec {
            *v /= norm;
        }
        vec
    }

    #[test]
    fn test_f16_quantization_and_dot_product() {
        let v1 = sample_l2_normalized_vector(1.0, 384);
        let v2 = sample_l2_normalized_vector(2.0, 384);

        let blob2 = SqliteVectorIndex::vector_to_f16_blob(&v2);
        assert_eq!(blob2.len(), 384 * 2);

        let recovered_v2 = SqliteVectorIndex::f16_blob_to_vector(&blob2);
        assert_eq!(recovered_v2.len(), 384);

        // Exact dot product with f32
        let exact_dot: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
        let f16_dot = SqliteVectorIndex::dot_product_f32_f16(&v1, &blob2);

        // Precision difference between f32 and f16 should be < 0.001
        assert!((exact_dot - f16_dot).abs() < 0.001);
    }

    #[test]
    fn test_sqlite_vector_index_crud_and_search() {
        let db = Database::open_in_memory().unwrap();

        // Insert prerequisite files and chunks for foreign key integrity
        {
            let conn = db.writer();
            conn.execute_batch(
                "INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (10, '/test/doc1.txt', '/test', 'doc1.txt', 'txt', 100, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
                 INSERT INTO files (id, path, parent_dir, name, ext, size, mtime, ctime, kind, status, last_indexed_at)
                 VALUES (20, '/test/doc2.txt', '/test', 'doc2.txt', 'txt', 200, '2024-01-01', '2024-01-01', 'document', 'active', '2024-01-01');
                 INSERT INTO chunks (id, file_id, ordinal, text, start, end)
                 VALUES (1, 10, 0, 'sample chunk 1', 0, 14);
                 INSERT INTO chunks (id, file_id, ordinal, text, start, end)
                 VALUES (2, 10, 1, 'sample chunk 2', 15, 29);
                 INSERT INTO chunks (id, file_id, ordinal, text, start, end)
                 VALUES (3, 20, 0, 'sample chunk 3', 0, 14);"
            ).unwrap();
        }

        let index = SqliteVectorIndex::new(db);

        let v1 = sample_l2_normalized_vector(1.0, 384);
        let v2 = sample_l2_normalized_vector(1.05, 384); // very similar to v1
        let v3 = sample_l2_normalized_vector(5.0, 384); // different

        let r1 = VectorRecord {
            chunk_id: 1,
            file_id: 10,
            model_id: "bge-small-en-v1.5".into(),
            text_hash: "hash1".into(),
            vector: v1.clone(),
        };
        let r2 = VectorRecord {
            chunk_id: 2,
            file_id: 10,
            model_id: "bge-small-en-v1.5".into(),
            text_hash: "hash2".into(),
            vector: v2.clone(),
        };
        let r3 = VectorRecord {
            chunk_id: 3,
            file_id: 20,
            model_id: "bge-small-en-v1.5".into(),
            text_hash: "hash3".into(),
            vector: v3.clone(),
        };

        index.insert_batch(&[r1, r2, r3]).unwrap();
        assert_eq!(index.count(None).unwrap(), 3);
        assert_eq!(index.count(Some("bge-small-en-v1.5")).unwrap(), 3);
        assert_eq!(index.count(Some("other-model")).unwrap(), 0);

        // Search with v1
        let matches = index.search(&v1, 2, Some("bge-small-en-v1.5")).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].chunk_id, 1);
        assert!((matches[0].score - 1.0).abs() < 0.005);
        assert_eq!(matches[1].chunk_id, 2);

        // Test chunk hashes retrieval
        let hashes = index
            .get_indexed_chunk_hashes(10, "bge-small-en-v1.5")
            .unwrap();
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes.get(&1).unwrap(), "hash1");
        assert_eq!(hashes.get(&2).unwrap(), "hash2");

        // Delete for file
        let deleted = index.delete_for_file(10).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(index.count(None).unwrap(), 1);
    }
}
