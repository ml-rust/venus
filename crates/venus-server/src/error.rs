//! Error types for Venus server.

use std::path::PathBuf;

use venus_core::graph::CellId;

/// Server error type.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// IO error.
    #[error("IO error at {path}: {message}")]
    Io { path: PathBuf, message: String },

    /// Venus core error.
    #[error("Core error: {0}")]
    Core(#[from] venus_core::Error),

    /// Cell not found.
    #[error("Cell not found: {0:?}")]
    CellNotFound(CellId),

    /// Execution already in progress.
    #[error("Execution already in progress")]
    ExecutionInProgress,

    /// WebSocket error.
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// JSON serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Watch error.
    #[error("File watch error: {0}")]
    Watch(String),

    /// Execution was aborted by user request.
    #[error("Execution aborted")]
    ExecutionAborted,

    /// Execution timed out.
    #[error("Execution timed out")]
    ExecutionTimeout,

    /// Invalid operation.
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

impl From<std::io::Error> for ServerError {
    fn from(e: std::io::Error) -> Self {
        Self::Io {
            path: PathBuf::new(),
            message: e.to_string(),
        }
    }
}

/// Result type for server operations.
pub type ServerResult<T> = Result<T, ServerError>;
