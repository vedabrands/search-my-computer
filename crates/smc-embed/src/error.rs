use rusqlite::types::FromSqlError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EmbedError {
    #[error("Model files not found: {0}")]
    ModelNotFound(String),

    #[error("Failed to load model: {0}")]
    ModelLoad(String),

    #[error("Tokenizer error: {0}")]
    Tokenizer(String),

    #[error("ONNX inference error: {0}")]
    Inference(String),

    #[error("Dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Database conversion error: {0}")]
    FromSql(#[from] FromSqlError),

    #[error("Core error: {0}")]
    Core(#[from] smc_core::error::CoreError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Other embed error: {0}")]
    Other(String),
}

pub type EmbedResult<T> = Result<T, EmbedError>;
