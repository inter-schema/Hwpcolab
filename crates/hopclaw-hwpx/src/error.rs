use hopclaw_types::{ErrorCode, ErrorModel};
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

    #[error("unsupported operation on HWPX: {0}")]
    UnsupportedOperation(String),

    #[error("missing required package part: {0}")]
    MissingPart(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<HwpxError> for ErrorModel {
    fn from(err: HwpxError) -> Self {
        let code = match &err {
            HwpxError::TableNotFound { .. } => ErrorCode::E_TABLE_NOT_FOUND,
            HwpxError::NestedTableDepthExceeded { .. } | HwpxError::UnsupportedOperation(_) => {
                ErrorCode::E_OP_UNSUPPORTED_FORMAT
            }
            HwpxError::InvalidPackage(_)
            | HwpxError::Zip(_)
            | HwpxError::Xml(_)
            | HwpxError::Utf8(_)
            | HwpxError::MissingPart(_)
            | HwpxError::OutOfRange(_)
            | HwpxError::InvalidArgument(_)
            | HwpxError::Io(_) => ErrorCode::E_DOC_INVALID,
        };
        ErrorModel::new(code, err.to_string())
    }
}
