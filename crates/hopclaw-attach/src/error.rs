use thiserror::Error;

#[derive(Debug, Error)]
pub enum AttachError {
    #[error("{0}")]
    LimitExceeded(String),

    #[error("{0}")]
    InvalidArchive(String),

    #[error("{0}")]
    ParseFailed(String),

    #[error("{0}")]
    UnsupportedFeature(String),

    #[error("{0}")]
    WorkerTimeout(String),

    #[error("{0}")]
    WorkerFailed(String),

    #[error("{0}")]
    Io(#[from] std::io::Error),
}
