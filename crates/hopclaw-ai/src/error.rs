use thiserror::Error;

/// Unified error type for `hopclaw-ai` provider adapters, endpoint validation,
/// HTTP proxy execution, and secret storage.
#[derive(Debug, Error)]
pub enum AiError {
    #[error("{0}")]
    ToolsUnsupported(String),

    #[error("{message}")]
    TransientProvider {
        message: String,
        status: Option<u16>,
    },

    #[error("{0}")]
    EndpointPolicy(String),

    #[error("{0}")]
    HttpProxy(String),

    #[error("{0}")]
    Cancelled(String),

    #[error("The operation was aborted.")]
    Aborted,

    #[error("{0}")]
    Provider(String),

    #[error("{0}")]
    InvalidArgs(String),

    #[error("{0}")]
    SecretStore(String),
}
