pub mod embedder;
pub mod error;
pub mod hash;
pub mod processor;
pub mod vector_index;

pub use embedder::{DEFAULT_MODEL_ID, Embedder, EmbeddingModelConfig, LITE_MODEL_ID, OnnxEmbedder};
pub use error::{EmbedError, EmbedResult};
pub use hash::hash_chunk_text;
pub use processor::{EmbedJobResult, enqueue_missing_embeddings, process_file_embedding};
pub use vector_index::{SqliteVectorIndex, VectorIndex, VectorMatch, VectorRecord};
