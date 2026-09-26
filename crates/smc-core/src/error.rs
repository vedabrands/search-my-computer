use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("database pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("scanner error: {0}")]
    Scanner(String),

    #[error("job error: {0}")]
    Job(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
