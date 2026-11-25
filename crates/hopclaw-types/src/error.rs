use serde::{Deserialize, Serialize};
use std::fmt;

/// Canonical machine-readable error codes shared across all HOP-Claw Rust crates and IPC boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum ErrorCode {
    E_DOC_INVALID,
    E_TABLE_NOT_FOUND,
    E_OP_UNSUPPORTED_FORMAT,
    E_GUARD_REJECTED,
    E_STALE_REVISION,
    E_TIMEOUT,
    E_CANCELLED,
    E_LLM_HTTP,
    E_ENDPOINT_POLICY,
    E_ATTACHMENT_LIMIT,
    E_INTERNAL_PANIC,
}

impl ErrorCode {
    /// Returns the wire string representation (`"E_DOC_INVALID"`, etc.).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::E_DOC_INVALID => "E_DOC_INVALID",
            Self::E_TABLE_NOT_FOUND => "E_TABLE_NOT_FOUND",
            Self::E_OP_UNSUPPORTED_FORMAT => "E_OP_UNSUPPORTED_FORMAT",
            Self::E_GUARD_REJECTED => "E_GUARD_REJECTED",
            Self::E_STALE_REVISION => "E_STALE_REVISION",
            Self::E_TIMEOUT => "E_TIMEOUT",
            Self::E_CANCELLED => "E_CANCELLED",
            Self::E_LLM_HTTP => "E_LLM_HTTP",
            Self::E_ENDPOINT_POLICY => "E_ENDPOINT_POLICY",
            Self::E_ATTACHMENT_LIMIT => "E_ATTACHMENT_LIMIT",
            Self::E_INTERNAL_PANIC => "E_INTERNAL_PANIC",
        }
    }

    /// Default retryability hint for this error category.
    pub const fn default_retryable(self) -> bool {
        matches!(self, Self::E_TIMEOUT | Self::E_LLM_HTTP)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structured error payload serialized across Tauri IPC and Axum devbridge boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("[{code}] {message}")]
pub struct ErrorModel {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub retryable: bool,
}

impl ErrorModel {
    /// Create a new `ErrorModel` using the default retryability for `code`.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            retryable: code.default_retryable(),
        }
    }

    /// Attach optional diagnostic detail.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Override the `retryable` flag.
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// Serialize this error model to JSON string for Tauri command `Err(String)` compatibility.
    pub fn to_ipc_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| self.to_string())
    }
}
