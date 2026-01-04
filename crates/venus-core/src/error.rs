//! Error types for venus-core.

use thiserror::Error;

/// Result type for venus-core operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur in venus-core.
#[derive(Debug, Error)]
pub enum Error {
    /// Failed to parse notebook source.
    #[error("parse error: {0}")]
    Parse(String),

    /// Cyclic dependency detected in the cell graph.
    #[error("cyclic dependency detected: {0}")]
    CyclicDependency(String),

    /// Cell not found.
    #[error("cell not found: {0}")]
    CellNotFound(String),

    /// Compilation failed.
    #[error("compilation failed{}: {message}", cell_id.as_ref().map(|id| format!(" for cell {}", id)).unwrap_or_default())]
    Compilation {
        cell_id: Option<String>,
        message: String,
    },

    /// Failed to load dynamic library.
    #[error("failed to load library: {0}")]
    LibraryLoad(#[from] libloading::Error),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Deserialization error.
    #[error("deserialization error: {0}")]
    Deserialization(String),

    /// Schema evolution error (incompatible type change).
    #[error("schema evolution error: {0}")]
    SchemaEvolution(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// IPC communication error with worker process.
    #[error("IPC error: {0}")]
    Ipc(String),

    /// Toolchain error.
    #[error("toolchain error: {0}")]
    Toolchain(String),

    /// Execution error.
    #[error("execution error: {0}")]
    Execution(String),

    /// Execution was aborted by user request.
    #[error("execution aborted")]
    Aborted,

    /// Invalid operation (e.g., moving first cell up).
    #[error("invalid operation: {0}")]
    InvalidOperation(String),
}

impl Error {
    /// Get a recovery suggestion for this error, if available.
    ///
    /// Returns a user-friendly hint on how to fix the error.
    pub fn recovery_hint(&self) -> Option<String> {
        match self {
            Error::CyclicDependency(msg) => {
                // Extract cycle path from error message if possible
                if msg.contains("→") {
                    Some("Remove one of the dependency edges in the cycle to break it. For example, if A → B → C → A, you could remove the dependency from C back to A.".to_string())
                } else {
                    Some("Review your cell dependencies and remove circular references.".to_string())
                }
            }
            Error::CellNotFound(msg) => {
                if msg.contains("depends on") {
                    Some("Check that the cell name matches exactly (case-sensitive). If the cell was renamed, update all dependencies that reference it.".to_string())
                } else {
                    Some("Verify the cell name is spelled correctly and the cell exists in your notebook.".to_string())
                }
            }
            Error::Compilation { message, .. } => {
                if message.contains("type mismatch") || message.contains("expected") {
                    Some("Check that parameter types match the output types of dependency cells. Use '&Type' for borrowed references, not 'Type'.".to_string())
                } else if message.contains("cannot find") {
                    Some("Ensure all required types and functions are imported. You may need to add dependencies to the notebook header.".to_string())
                } else {
                    Some("Run with RUST_LOG=venus=debug for detailed compiler output. Fix the compilation errors in your cell code.".to_string())
                }
            }
            Error::Deserialization(msg) => {
                if msg.contains("type mismatch") || msg.contains("check dependency types") {
                    Some("The cell's parameter types don't match the actual output types from dependencies. Ensure parameter types exactly match what the dependency cells return.".to_string())
                } else {
                    Some("Check that your data structures have proper rkyv serialization derives: #[derive(Archive, RkyvSerialize, RkyvDeserialize)]".to_string())
                }
            }
            Error::SchemaEvolution(msg) => {
                if msg.contains("breaking change") || msg.contains("incompatible") {
                    Some("You've changed a type definition in a way that's incompatible with cached data. Clean the cache with: rm -rf .venus/cache".to_string())
                } else {
                    Some("Type definitions have changed. Try cleaning the cache directory: rm -rf .venus/cache".to_string())
                }
            }
            Error::Toolchain(msg) => {
                if msg.contains("rustc") || msg.contains("not found") {
                    Some("Install Rust from https://rustup.rs if not already installed. Ensure 'rustc' is in your PATH.".to_string())
                } else if msg.contains("cranelift") {
                    Some("Cranelift backend is optional. Venus will fall back to standard rustc compilation.".to_string())
                } else {
                    Some("Verify your Rust installation with: rustc --version".to_string())
                }
            }
            Error::Execution(msg) => {
                if msg.contains("deserialize") || msg.contains("type") {
                    Some("Run with RUST_LOG=venus=debug to see detailed error information. Check that cell parameter types match dependency output types.".to_string())
                } else if msg.contains("panicked") {
                    Some("Check your cell code for unwrap() calls on None/Err values, array out-of-bounds access, or other panic sources. Add proper error handling.".to_string())
                } else {
                    None
                }
            }
            Error::Io(io_err) => {
                match io_err.kind() {
                    std::io::ErrorKind::NotFound => {
                        Some("Verify the file path is correct and the file exists.".to_string())
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        Some("Check file permissions. You may need to make the file readable/writable or run with appropriate permissions.".to_string())
                    }
                    std::io::ErrorKind::AlreadyExists => {
                        Some("The file already exists. Delete the existing file or choose a different name.".to_string())
                    }
                    _ => None,
                }
            }
            Error::Ipc(msg) => {
                if msg.contains("timeout") || msg.contains("disconnected") {
                    Some("The worker process may have crashed. Try cleaning the build directory: rm -rf .venus/build".to_string())
                } else {
                    None
                }
            }
            // These errors are self-explanatory or context-specific
            Error::Parse(_) | Error::LibraryLoad(_) | Error::Serialization(_) |
            Error::Aborted | Error::InvalidOperation(_) => None,
        }
    }

    /// Format the error with its recovery hint, if available.
    ///
    /// This is useful for displaying errors to users with actionable guidance.
    pub fn with_hint(&self) -> String {
        let base_msg = self.to_string();
        match self.recovery_hint() {
            Some(hint) => format!("{}\n\n💡 Hint: {}", base_msg, hint),
            None => base_msg,
        }
    }
}
