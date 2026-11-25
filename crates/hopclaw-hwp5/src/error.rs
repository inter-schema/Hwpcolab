use thiserror::Error;

/// Crate-wide result alias for HWP 5.0 operations.
pub type Result<T> = std::result::Result<T, Hwp5Error>;

/// Errors produced when opening, validating, transforming, or writing HWP 5.0 files.
#[derive(Debug, Error)]
pub enum Hwp5Error {
    #[error("E_IO: {0}")]
    Io(#[from] std::io::Error),

    #[error("E_INVALID_CONTAINER: {0}")]
    InvalidContainer(String),

    #[error("E_INVALID_HEADER: {0}")]
    InvalidHeader(String),

    #[error("E_COMPRESSION: {0}")]
    Compression(String),

    #[error("E_MALFORMED_RECORD: {0}")]
    MalformedRecord(String),

    #[error("E_MISSING_STREAM: stream not found: {0}")]
    MissingStream(String),

    #[error("E_DOCINFO_CORRUPT: {0}")]
    DocInfoCorrupt(String),

    #[error("E_TARGET_NOT_FOUND: {0}")]
    TargetNotFound(String),

    #[error("E_INVALID_TARGET: {0}")]
    InvalidTarget(String),

    #[error("E_OP_UNSUPPORTED_FORMAT: {0}")]
    UnsupportedFormat(String),

    #[error("E_NOT_IMPLEMENTED: {0}")]
    NotImplemented(String),
}
