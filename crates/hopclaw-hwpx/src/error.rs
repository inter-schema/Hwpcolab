use thiserror::Error;

#[derive(Debug, Error)]
pub enum HwpxError {
    #[error("invalid HWPX package: {0}")]
    InvalidPackage(String),

    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("xml error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("utf-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("table index {index} out of range (found {count})")]
    TableNotFound { index: usize, count: usize },

    #[error("index out of range: {0}")]
    OutOfRange(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("nested table depth limit exceeded: current depth is {depth}, max allowed parent depth is 1")]
    NestedTableDepthExceeded { depth: usize },

    #[error("missing required package part: {0}")]
    MissingPart(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
