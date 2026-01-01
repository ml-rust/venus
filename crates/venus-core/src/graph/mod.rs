//! Graph engine for dependency resolution.
//!
//! This module provides:
//! - Cell parsing from Rust source files
//! - Dependency graph construction
//! - Topological ordering for execution
//! - Cycle detection with helpful error messages

mod parser;
mod types;

pub use parser::CellParser;
pub use types::{CellId, CellInfo, Dependency, GraphEngine, SourceSpan};
