//! Salsa input types for Venus.
//!
//! Input types are the entry points for incremental computation.
//! Changes to inputs automatically invalidate dependent queries.

use std::path::PathBuf;
use std::sync::Arc;

use super::conversions::ExecutionStatus;

/// Input: Source file content.
///
/// This is the primary input to the incremental system.
/// When source text changes, all dependent queries are invalidated.
#[salsa::input]
pub struct SourceFile {
    /// Path to the source file
    pub path: PathBuf,

    /// Content of the source file
    pub text: String,
}

/// Input: Compiler settings for cell compilation.
///
/// This input provides the configuration needed for cell compilation.
/// Changes to settings (e.g., optimization level) will invalidate
/// compiled cell queries.
#[salsa::input]
pub struct CompilerSettings {
    /// Directory for build artifacts (.venus/build/)
    pub build_dir: PathBuf,

    /// Directory for cached outputs (.venus/cache/)
    pub cache_dir: PathBuf,

    /// Path to the compiled universe library
    pub universe_path: Option<PathBuf>,

    /// Use Cranelift backend (fast compilation)
    pub use_cranelift: bool,

    /// Optimization level (0-3)
    pub opt_level: u8,
}

/// Input: Cell execution outputs.
///
/// This input stores the execution status for all cells in a notebook.
/// It is updated after cells are executed, allowing Salsa to track
/// when outputs change.
///
/// Uses `Arc` to efficiently share the potentially large status map
/// without expensive cloning on every query.
#[salsa::input]
pub struct CellOutputs {
    /// Execution status for each cell, indexed by cell ID.
    /// Wrapped in Arc for efficient sharing.
    pub statuses: Arc<Vec<ExecutionStatus>>,

    /// Version counter that increments on any change.
    /// Used for quick staleness checks.
    pub version: u64,
}
