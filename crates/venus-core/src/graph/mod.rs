//! Graph engine for dependency resolution.
//!
//! This module provides:
//! - Cell parsing from Rust source files
//! - Dependency graph construction
//! - Topological ordering for execution
//! - Cycle detection with helpful error messages
//! - Source file editing for cell insertion, deletion, and reordering

mod parser;
mod source_editor;
mod types;

pub use parser::CellParser;
pub use source_editor::{MoveDirection, SourceEditor};
pub use types::{CellId, CellInfo, Dependency, GraphEngine, SourceSpan};
