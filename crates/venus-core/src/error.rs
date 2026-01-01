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
