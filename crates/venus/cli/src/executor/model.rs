use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use venus_core::compile::{CompiledCell, CompilerConfig, ToolchainManager, UniverseBuilder};
use venus_core::graph::{CellId, CellInfo};
use venus_core::paths::NotebookDirs;
use venus_core::state::BoxedOutput;

/// Compiled cell with metadata for execution.
#[derive(Clone)]
pub struct CompiledCellInfo {
    /// The compiled cell.
    pub compiled: CompiledCell,
    /// Number of dependencies.
    pub dep_count: usize,
    /// Compilation time in milliseconds.
    #[allow(dead_code)]
    pub compile_time_ms: u64,
    /// Whether the cell was cached.
    #[allow(dead_code)]
    pub cached: bool,
}

/// Result of cell compilation.
pub struct CompilationInfo {
    /// Successfully compiled cells.
    pub cells: HashMap<CellId, CompiledCellInfo>,
    /// Compilation errors by cell name.
    pub errors: Vec<(String, Vec<venus_core::compile::CompileError>)>,
}

/// Result of cell execution.
pub struct ExecutionInfo {
    /// Cells that were executed (in order).
    pub executed_cells: Vec<CellId>,
    /// Execution time.
    pub execution_time: Duration,
    /// Cell outputs (cell_id -> output).
    pub outputs: HashMap<CellId, Arc<BoxedOutput>>,
}

/// Notebook executor that manages the full execution pipeline.
pub struct NotebookExecutor {
    /// Absolute path to the notebook file.
    pub notebook_path: PathBuf,
    /// Notebook source code.
    #[allow(dead_code)]
    pub source: String,
    /// Notebook directories.
    pub dirs: NotebookDirs,
    /// Toolchain manager.
    pub toolchain: ToolchainManager,
    /// Parsed cells.
    pub cells: Vec<CellInfo>,
    /// Cell name to ID mapping.
    pub cell_ids: HashMap<String, CellId>,
    /// Topological execution order.
    pub order: Vec<CellId>,
    /// Dependency map (cell_id -> dependencies).
    pub deps: HashMap<CellId, Vec<CellId>>,
    /// Compiler configuration.
    pub config: CompilerConfig,
    /// Universe builder (for dependency hash).
    pub universe_builder: UniverseBuilder,
    /// Path to compiled universe.
    pub universe_path: PathBuf,
    /// Whether using release mode.
    #[allow(dead_code)]
    pub release: bool,
}
