//! Core engine for Venus reactive notebook environment.
//!
//! # ⚠️ API Stability Warning
//!
//! **This crate contains internal APIs that are UNSTABLE and may change without notice.**
//!
//! For notebook development, use the `venus` crate instead:
//! ```rust,ignore
//! use venus::prelude::*;  // STABLE user-facing API
//! ```
//!
//! The `venus-core` APIs are intended for:
//! - Building custom notebook tools and extensions
//! - Advanced integrations with Venus internals
//! - Contributing to Venus development
//!
//! **Stability guarantees:**
//! - ❌ **No SemVer guarantees** - breaking changes may occur in minor versions (0.x.y)
//! - ❌ **No deprecation warnings** - APIs may be removed without warning
//! - ❌ **Internal implementation details** - subject to refactoring
//!
//! If you need stable APIs, please open an issue describing your use case.
//!
//! ---
//!
//! This crate provides:
//! - Graph engine for dependency resolution
//! - Compilation pipeline (Cranelift JIT + LLVM)
//! - State management with schema evolution
//! - Salsa-based incremental computation
//! - Cell execution and hot-reload

pub mod compile;
pub mod error;
pub mod execute;
pub mod graph;
pub mod ipc;
pub mod paths;
pub mod salsa_db;
pub mod state;
pub mod widgets;

pub use error::{Error, Result};
pub use paths::NotebookDirs;
pub use execute::{
    CellContext, ExecutionCallback, HotReloader, LinearExecutor, LoadedCell, ParallelExecutor,
    ProcessExecutor, WindowsDllHandler,
};
pub use graph::{CellId, CellInfo, CellParser, Dependency, GraphEngine};
pub use salsa_db::{
    CellData, CellOutputData, CellOutputs, CompilationStatus, CompiledCellData, CompilerSettings,
    ExecutionStatus, GraphAnalysis, QueryResult, SourceFile, VenusDatabase, all_cells_executed,
    cell_names, cell_output, cell_output_data, compile_all_cells, compiled_cell, dependency_hash,
    execution_order, execution_order_result, graph_analysis, graph_analysis_result, invalidated_by,
    parallel_levels, parse_cells, parse_cells_result,
};
pub use state::{CellOutput, SchemaChange, StateManager, TypeFingerprint, ZeroCopyOutput};
