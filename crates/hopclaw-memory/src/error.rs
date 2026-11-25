use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("{0}")]
    Validation(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Storage(String),

    #[error("{0}")]
    Llm(String),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Json(#[from] serde_json::Error),
}
