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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ServerError::CellNotFound(CellId::new(42));
        assert_eq!(err.to_string(), "Cell not found: CellId(42)");

        let err = ServerError::ExecutionInProgress;
        assert_eq!(err.to_string(), "Execution already in progress");

        let err = ServerError::ExecutionAborted;
        assert_eq!(err.to_string(), "Execution aborted");

        let err = ServerError::ExecutionTimeout;
        assert_eq!(err.to_string(), "Execution timed out");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let server_err: ServerError = io_err.into();

        match server_err {
            ServerError::Io { path, message } => {
                assert_eq!(path, PathBuf::new());
                assert!(message.contains("file not found"));
            }
            _ => panic!("Expected Io error"),
        }
    }

    #[test]
    fn test_json_error_conversion() {
        let json_str = "{invalid json";
        let json_err = serde_json::from_str::<serde_json::Value>(json_str).unwrap_err();
        let server_err: ServerError = json_err.into();

        assert!(matches!(server_err, ServerError::Json(_)));
    }

    #[test]
    fn test_core_error_conversion() {
        // Test that venus_core errors convert properly
        let core_err = venus_core::Error::CellNotFound("test_cell".to_string());
        let server_err: ServerError = core_err.into();

        assert!(matches!(server_err, ServerError::Core(_)));
        assert!(server_err.to_string().contains("test_cell"));
    }

    #[test]
    fn test_custom_errors() {
        let err = ServerError::Io {
            path: PathBuf::from("/test/file.rs"),
            message: "Permission denied".to_string(),
        };
        assert!(err.to_string().contains("/test/file.rs"));
        assert!(err.to_string().contains("Permission denied"));

        let err = ServerError::Watch("debouncer failed".to_string());
        assert_eq!(err.to_string(), "File watch error: debouncer failed");

        let err = ServerError::WebSocket("connection closed".to_string());
        assert_eq!(err.to_string(), "WebSocket error: connection closed");

        let err = ServerError::InvalidOperation("Cannot delete cell with dependencies".to_string());
        assert!(err
            .to_string()
            .contains("Cannot delete cell with dependencies"));
    }
}
