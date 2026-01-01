//! Error types for the sync engine.

use std::path::PathBuf;

/// Result type for sync operations.
pub type SyncResult<T> = Result<T, SyncError>;

/// Errors that can occur during sync operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Failed to read source file.
    #[error("Failed to read file {path}: {message}")]
    ReadError { path: PathBuf, message: String },

    /// Failed to write output file.
    #[error("Failed to write file {path}: {message}")]
    WriteError { path: PathBuf, message: String },

    /// Failed to parse RS file.
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Failed to serialize/deserialize JSON.
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// I/O error.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid notebook structure.
    #[error("Invalid notebook: {0}")]
    InvalidNotebook(String),
}
