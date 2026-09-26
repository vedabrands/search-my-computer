use crate::error::{EmbedError, EmbedResult};
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokenizers::Tokenizer;
use tracing::{debug, info};

pub const DEFAULT_MODEL_ID: &str = "bge-small-en-v1.5";
pub const LITE_MODEL_ID: &str = "all-MiniLM-L6-v2";

/// Simple, thread-safe LRU cache for query embeddings.
#[derive(Debug, Clone)]
pub struct QueryEmbeddingCache {
    capacity: usize,
    map: HashMap<String, Vec<f32>>,
    order: VecDeque<String>,
}

impl QueryEmbeddingCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
        }
    }

    pub fn get(&mut self, query: &str) -> Option<Vec<f32>> {
        if let Some(vec) = self.map.get(query) {
            if let Some(pos) = self.order.iter().position(|q| q == query) {
                self.order.remove(pos);
                self.order.push_back(query.to_string());
            }
            Some(vec.clone())
        } else {
            None
        }
    }

    pub fn insert(&mut self, query: String, vector: Vec<f32>) {
        if self.map.contains_key(&query) {
            self.map.insert(query.clone(), vector);
            if let Some(pos) = self.order.iter().position(|q| q == &query) {
                self.order.remove(pos);
            }
            self.order.push_back(query);
            return;
        }

        while self.order.len() >= self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }

        self.order.push_back(query.clone());
        self.map.insert(query, vector);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

/// Pooling strategy for extracting a sentence embedding from token embeddings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolingMode {
    /// Mean pooling over all non-padding tokens (weighted by attention mask).
    Mean,
    /// [CLS] token embedding (first token).
    Cls,
}

/// Configuration for an embedding model.
#[derive(Debug, Clone)]
pub struct EmbeddingModelConfig {
    pub model_id: String,
    pub model_path: PathBuf,
    pub tokenizer_path: PathBuf,
    pub dimension: usize,
    pub max_sequence_length: usize,
    pub query_prefix: String,
    pub document_prefix: String,
    pub num_threads: usize,
    pub idle_unload_secs: u64,
    pub pooling: PoolingMode,
}

impl EmbeddingModelConfig {
    /// Configuration for default bge-small-en-v1.5 model.
    pub fn default_bge_small(models_dir: &Path) -> Self {
        let dir = models_dir.join(DEFAULT_MODEL_ID);
        Self {
            model_id: DEFAULT_MODEL_ID.to_string(),
            model_path: dir.join("model.onnx"),
            tokenizer_path: dir.join("tokenizer.json"),
            dimension: 384,
            max_sequence_length: 256,
            query_prefix: "Represent this sentence for searching relevant passages: ".to_string(),
            document_prefix: String::new(),
            num_threads: 2,
            idle_unload_secs: 300, // 5 minutes
            pooling: PoolingMode::Mean,
        }
    }

    /// Configuration for lite all-MiniLM-L6-v2 model.
    pub fn lite_minilm(models_dir: &Path) -> Self {
        let dir = models_dir.join(LITE_MODEL_ID);
        Self {
            model_id: LITE_MODEL_ID.to_string(),
            model_path: dir.join("model.onnx"),
            tokenizer_path: dir.join("tokenizer.json"),
            dimension: 384,
            max_sequence_length: 256,
            query_prefix: String::new(),
            document_prefix: String::new(),
            num_threads: 2,
            idle_unload_secs: 300, // 5 minutes
            pooling: PoolingMode::Mean,
        }
    }
}

/// Trait abstracting embedding generation for documents and queries.
pub trait Embedder: Send + Sync {
    /// Generates L2-normalized embeddings for a batch of document texts.
    fn embed_documents(&self, docs: &[&str]) -> EmbedResult<Vec<Vec<f32>>>;

    /// Generates an L2-normalized embedding for a search query.
    fn embed_query(&self, query: &str) -> EmbedResult<Vec<f32>>;

    /// Returns the embedding vector dimension.
    fn dims(&self) -> usize;

    /// Returns the unique model identifier.
    fn model_id(&self) -> &str;

    /// Unloads the model from memory if currently loaded.
    fn unload(&self);

    /// Checks if the model is currently resident in memory.
    fn is_loaded(&self) -> bool;

    /// Checks if the model should be unloaded due to idle time and unloads if needed.
    fn maybe_unload_idle(&self) -> bool;
}

struct LoadedSession {
    session: Session,
    tokenizer: Tokenizer,
    input_names: Vec<String>,
}

/// ONNX Runtime-backed local embedder with dynamic batching, lazy loading, and idle auto-unload.
pub struct OnnxEmbedder {
    config: EmbeddingModelConfig,
    session: Arc<Mutex<Option<LoadedSession>>>,
    last_used: Arc<Mutex<Instant>>,
    query_cache: Arc<Mutex<QueryEmbeddingCache>>,
}

impl OnnxEmbedder {
    /// Creates a new OnnxEmbedder with lazy model loading.
    pub fn new(config: EmbeddingModelConfig) -> Self {
        Self {
            config,
            session: Arc::new(Mutex::new(None)),
            last_used: Arc::new(Mutex::new(Instant::now())),
            query_cache: Arc::new(Mutex::new(QueryEmbeddingCache::new(128))),
        }
    }

    /// Returns a reference to the configuration.
    pub fn config(&self) -> &EmbeddingModelConfig {
        &self.config
    }

    /// Ensures the model and tokenizer are loaded into memory.
    fn get_or_load_session(
        &self,
    ) -> EmbedResult<parking_lot::MutexGuard<'_, Option<LoadedSession>>> {
        let mut guard = self.session.lock();
        if guard.is_none() {
            if !self.config.model_path.exists() {
                return Err(EmbedError::ModelNotFound(format!(
                    "ONNX model file not found at {:?}",
                    self.config.model_path
                )));
            }
            if !self.config.tokenizer_path.exists() {
                return Err(EmbedError::ModelNotFound(format!(
                    "Tokenizer file not found at {:?}",
                    self.config.tokenizer_path
                )));
            }

            info!(
                model_id = %self.config.model_id,
                threads = self.config.num_threads,
                "loading ONNX embedding session"
            );

            let mut tokenizer = Tokenizer::from_file(&self.config.tokenizer_path)
                .map_err(|e| EmbedError::Tokenizer(format!("failed to load tokenizer: {e}")))?;

            // Configure padding and truncation on tokenizer
            let params = tokenizers::PaddingParams {
                strategy: tokenizers::PaddingStrategy::BatchLongest,
                pad_to_multiple_of: None,
                pad_id: 0,
                pad_type_id: 0,
                pad_token: "[PAD]".to_string(),
                direction: tokenizers::PaddingDirection::Right,
            };
            tokenizer.with_padding(Some(params));

            let trunc = tokenizers::TruncationParams {
                max_length: self.config.max_sequence_length,
                strategy: tokenizers::TruncationStrategy::LongestFirst,
                stride: 0,
                direction: tokenizers::TruncationDirection::Right,
            };
            tokenizer
                .with_truncation(Some(trunc))
                .map_err(|e| EmbedError::Tokenizer(format!("truncation config error: {e}")))?;

            let session = Session::builder()
                .map_err(|e| EmbedError::ModelLoad(format!("session builder failed: {e}")))?
                .with_intra_threads(self.config.num_threads)
                .map_err(|e| EmbedError::ModelLoad(format!("setting threads failed: {e}")))?
                .commit_from_memory(&std::fs::read(&self.config.model_path)?)
                .map_err(|e| {
                    EmbedError::ModelLoad(format!("failed to load model from memory: {e}"))
                })?;

            let input_names: Vec<String> = session
                .inputs()
                .iter()
                .map(|inp| inp.name().to_string())
                .collect();

            debug!(?input_names, "session loaded successfully");

            *guard = Some(LoadedSession {
                session,
                tokenizer,
                input_names,
            });
        }

        *self.last_used.lock() = Instant::now();
        Ok(guard)
    }

    /// L2 normalize a slice of floats in-place.
    fn l2_normalize(vec: &mut [f32]) {
        let sum_sq: f32 = vec.iter().map(|&x| x * x).sum();
        let norm = sum_sq.sqrt();
        if norm > 1e-12 {
            for x in vec.iter_mut() {
                *x /= norm;
            }
        }
    }

    /// Performs inference over a batch of preprocessed strings.
    fn infer_batch(&self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut session_guard = self.get_or_load_session()?;
        let loaded = session_guard
            .as_mut()
            .ok_or_else(|| EmbedError::ModelLoad("session not available".into()))?;

        // 1. Tokenize batch
        let encodings = loaded
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| EmbedError::Tokenizer(format!("tokenization failed: {e}")))?;

        let batch_size = encodings.len();
        if batch_size == 0 {
            return Ok(Vec::new());
        }

        let seq_len = encodings[0].get_ids().len();
        let total_elements = batch_size * seq_len;

        let mut input_ids = Vec::with_capacity(total_elements);
        let mut attention_mask = Vec::with_capacity(total_elements);
        let mut token_type_ids = Vec::with_capacity(total_elements);

        for enc in &encodings {
            for &id in enc.get_ids() {
                input_ids.push(id as i64);
            }
            for &mask in enc.get_attention_mask() {
                attention_mask.push(mask as i64);
            }
            for &type_id in enc.get_type_ids() {
                token_type_ids.push(type_id as i64);
            }
        }

        let shape = vec![batch_size as i64, seq_len as i64];

        let input_ids_tensor = Tensor::from_array((shape.clone(), input_ids))
            .map_err(|e| EmbedError::Inference(format!("input_ids tensor creation: {e}")))?;

        let attention_mask_tensor = Tensor::from_array((shape.clone(), attention_mask.clone()))
            .map_err(|e| EmbedError::Inference(format!("attention_mask tensor creation: {e}")))?;

        // Build inputs based on model requirements
        let mut session_inputs = ort::inputs![
            "input_ids" => input_ids_tensor,
            "attention_mask" => attention_mask_tensor,
        ];

        let has_token_type_ids = loaded
            .input_names
            .iter()
            .any(|name| name == "token_type_ids");

        if has_token_type_ids {
            let token_type_ids_tensor =
                Tensor::from_array((shape, token_type_ids)).map_err(|e| {
                    EmbedError::Inference(format!("token_type_ids tensor creation: {e}"))
                })?;
            session_inputs.push(("token_type_ids".into(), token_type_ids_tensor.into()));
        }

        // 2. Run ONNX Inference
        let outputs = loaded
            .session
            .run(session_inputs)
            .map_err(|e| EmbedError::Inference(format!("session execution failed: {e}")))?;

        // Output tensor extraction (typically first output is last_hidden_state or sentence_embedding)
        let (_output_name, output_value) = outputs
            .into_iter()
            .next()
            .ok_or_else(|| EmbedError::Inference("no output produced by model".into()))?;

        let (out_shape, out_data) = output_value
            .try_extract_tensor::<f32>()
            .map_err(|e| EmbedError::Inference(format!("failed to extract output tensor: {e}")))?;

        // Check if output is 3D (batch_size, seq_len, hidden_dim) or 2D (batch_size, hidden_dim)
        let hidden_dim = self.config.dimension;
        let mut embeddings = Vec::with_capacity(batch_size);

        if out_shape.len() == 3 {
            // Shape: [batch, seq_len, hidden_dim] -> perform pooling
            let out_seq_len = out_shape[1] as usize;
            let out_hidden_dim = out_shape[2] as usize;

            if out_hidden_dim != hidden_dim {
                return Err(EmbedError::DimensionMismatch {
                    expected: hidden_dim,
                    actual: out_hidden_dim,
                });
            }

            for (b, enc) in encodings.iter().enumerate() {
                let mask = enc.get_attention_mask();
                let mut emb = vec![0.0f32; hidden_dim];

                match self.config.pooling {
                    PoolingMode::Mean => {
                        let mut sum_mask = 0.0f32;
                        for s in 0..out_seq_len {
                            let m = mask.get(s).copied().unwrap_or(0) as f32;
                            if m > 0.0 {
                                sum_mask += m;
                                let offset = (b * out_seq_len + s) * hidden_dim;
                                for h in 0..hidden_dim {
                                    emb[h] += out_data[offset + h] * m;
                                }
                            }
                        }
                        if sum_mask > 0.0 {
                            for val in emb.iter_mut().take(hidden_dim) {
                                *val /= sum_mask;
                            }
                        }
                    }
                    PoolingMode::Cls => {
                        let offset = b * out_seq_len * hidden_dim;
                        emb[..hidden_dim].copy_from_slice(&out_data[offset..(offset + hidden_dim)]);
                    }
                }

                Self::l2_normalize(&mut emb);
                embeddings.push(emb);
            }
        } else if out_shape.len() == 2 {
            // Pre-pooled output: [batch, hidden_dim]
            let out_hidden_dim = out_shape[1] as usize;
            if out_hidden_dim != hidden_dim {
                return Err(EmbedError::DimensionMismatch {
                    expected: hidden_dim,
                    actual: out_hidden_dim,
                });
            }

            for b in 0..batch_size {
                let offset = b * hidden_dim;
                let mut emb = out_data[offset..offset + hidden_dim].to_vec();
                Self::l2_normalize(&mut emb);
                embeddings.push(emb);
            }
        } else {
            return Err(EmbedError::Inference(format!(
                "unexpected model output shape: {:?}",
                out_shape
            )));
        }

        Ok(embeddings)
    }
}

impl Embedder for OnnxEmbedder {
    fn embed_documents(&self, docs: &[&str]) -> EmbedResult<Vec<Vec<f32>>> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }

        let prefix = &self.config.document_prefix;
        let prepared: Vec<String> = docs
            .iter()
            .map(|doc| {
                if prefix.is_empty() {
                    doc.to_string()
                } else {
                    format!("{}{}", prefix, doc)
                }
            })
            .collect();

        // Process in chunks of 32 for dynamic batching without overloading memory
        const BATCH_SIZE: usize = 32;
        let mut results = Vec::with_capacity(docs.len());

        for chunk in prepared.chunks(BATCH_SIZE) {
            let chunk_embs = self.infer_batch(chunk)?;
            results.extend(chunk_embs);
        }

        Ok(results)
    }

    fn embed_query(&self, query: &str) -> EmbedResult<Vec<f32>> {
        let norm_query = query.trim().to_string();
        if norm_query.is_empty() {
            return Err(EmbedError::Inference("empty query string".into()));
        }

        // Check in-memory LRU cache first
        if let Some(cached) = self.query_cache.lock().get(&norm_query) {
            debug!(query = %norm_query, "query embedding cache hit");
            return Ok(cached);
        }

        let prefix = &self.config.query_prefix;
        let prepared = if prefix.is_empty() {
            norm_query.clone()
        } else {
            format!("{}{}", prefix, norm_query)
        };

        let mut embs = self.infer_batch(&[prepared])?;
        let emb = embs
            .pop()
            .ok_or_else(|| EmbedError::Inference("no embedding generated for query".into()))?;

        // Cache the newly computed query embedding
        self.query_cache.lock().insert(norm_query, emb.clone());
        Ok(emb)
    }

    fn dims(&self) -> usize {
        self.config.dimension
    }

    fn model_id(&self) -> &str {
        &self.config.model_id
    }

    fn unload(&self) {
        let mut guard = self.session.lock();
        if guard.is_some() {
            info!(model_id = %self.config.model_id, "unloading ONNX embedding session to free memory");
            *guard = None;
        }
    }

    fn is_loaded(&self) -> bool {
        self.session.lock().is_some()
    }

    fn maybe_unload_idle(&self) -> bool {
        let last = *self.last_used.lock();
        if last.elapsed().as_secs() >= self.config.idle_unload_secs && self.is_loaded() {
            self.unload();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_l2_normalization() {
        let mut v = vec![3.0, 4.0];
        OnnxEmbedder::l2_normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert!((v[1] - 0.8).abs() < 1e-6);
        let norm: f32 = (v[0] * v[0] + v[1] * v[1]).sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_model_missing_error() {
        let config = EmbeddingModelConfig {
            model_id: "non-existent".into(),
            model_path: PathBuf::from("non/existent/model.onnx"),
            tokenizer_path: PathBuf::from("non/existent/tokenizer.json"),
            dimension: 384,
            max_sequence_length: 256,
            query_prefix: "".into(),
            document_prefix: "".into(),
            num_threads: 2,
            idle_unload_secs: 300,
            pooling: PoolingMode::Mean,
        };

        let embedder = OnnxEmbedder::new(config);
        assert!(!embedder.is_loaded());
        let res = embedder.embed_query("test");
        assert!(res.is_err());
        match res.unwrap_err() {
            EmbedError::ModelNotFound(_) => {}
            other => panic!("expected ModelNotFound error, got: {:?}", other),
        }
    }

    #[test]
    fn test_query_embedding_lru_cache() {
        let mut cache = QueryEmbeddingCache::new(2);
        assert!(cache.get("q1").is_none());

        cache.insert("q1".to_string(), vec![1.0, 0.0]);
        cache.insert("q2".to_string(), vec![0.0, 1.0]);

        assert_eq!(cache.get("q1"), Some(vec![1.0, 0.0]));
        assert_eq!(cache.get("q2"), Some(vec![0.0, 1.0]));

        // Access q1 to make q2 the oldest
        let _ = cache.get("q1");

        // Insert q3, which should evict q2
        cache.insert("q3".to_string(), vec![0.5, 0.5]);

        assert_eq!(cache.get("q1"), Some(vec![1.0, 0.0]));
        assert_eq!(cache.get("q2"), None);
        assert_eq!(cache.get("q3"), Some(vec![0.5, 0.5]));

        cache.clear();
        assert!(cache.get("q1").is_none());
        assert!(cache.get("q3").is_none());
    }
}
