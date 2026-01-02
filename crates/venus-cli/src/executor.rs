//! Shared notebook execution pipeline for Venus CLI.
//!
//! This module provides a unified execution pipeline used by `run`, `watch`, and `export` commands.
//! It handles the common workflow of parsing, compiling, and executing notebook cells.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use venus_core::compile::{
    CellCompiler, CompiledCell, CompilationResult, CompilerConfig, ToolchainManager,
    UniverseBuilder,
};
use venus_core::execute::{ExecutionCallback, LinearExecutor};
use venus_core::graph::{CellId, CellInfo, CellParser, GraphEngine};
use venus_core::paths::NotebookDirs;
use venus_core::state::{BoxedOutput, StateManager};
use venus_core::Error;

use crate::colors;

/// Progress callback that prints execution status to the terminal.
pub struct ProgressCallback {
    /// Whether to show verbose output.
    verbose: bool,
}

impl ProgressCallback {
    /// Create a new progress callback.
    pub fn new() -> Self {
        Self { verbose: false }
    }

    /// Create a verbose progress callback.
    #[allow(dead_code)]
    pub fn verbose() -> Self {
        Self { verbose: true }
    }
}

impl Default for ProgressCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionCallback for ProgressCallback {
    fn on_cell_started(&self, _cell_id: CellId, name: &str) {
        print!(
            "{}  ▶ Running{} {}{}... ",
            colors::CYAN,
            colors::RESET,
            colors::BOLD,
            name
        );
        colors::flush_stdout();
    }

    fn on_cell_completed(&self, _cell_id: CellId, _name: &str) {
        println!("{}✓{}", colors::GREEN, colors::RESET);
    }

    fn on_cell_error(&self, _cell_id: CellId, _name: &str, error: &Error) {
        println!("{}✗{}", colors::RED, colors::RESET);
        eprintln!("{}    Error:{} {}", colors::RED, colors::RESET, error);
    }

    fn on_level_started(&self, level: usize, cell_count: usize) {
        if self.verbose && cell_count > 1 {
            println!(
                "{}Level {}:{} {} cells (parallel)",
                colors::DIM,
                level,
                colors::RESET,
                cell_count
            );
        }
    }

    fn on_level_completed(&self, _level: usize) {}
}

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

impl NotebookExecutor {
    /// Create a new notebook executor.
    ///
    /// This performs the setup phase: parsing, dependency resolution, and universe building.
    pub fn new(notebook_path: &str, release: bool) -> anyhow::Result<Self> {
        let path = Path::new(notebook_path);
        if !path.exists() {
            anyhow::bail!("Notebook not found: {}", notebook_path);
        }

        let source = fs::read_to_string(path)?;
        let abs_path = path.canonicalize()?;
        let dirs = NotebookDirs::from_notebook_path(&abs_path)?;

        // Initialize toolchain
        Self::print_step("Checking toolchain");
        let toolchain = ToolchainManager::new()?;
        Self::print_success(None);

        // Parse cells
        Self::print_step("Parsing cells");
        let mut parser = CellParser::new();
        let parse_result = parser.parse_file(&abs_path)?;
        let cells = parse_result.code_cells;
        Self::print_success(Some(&format!("{} code cells", cells.len())));

        // Build dependency graph
        Self::print_step("Building dependency graph");
        let mut graph = GraphEngine::new();
        let mut cell_ids: HashMap<String, CellId> = HashMap::new();
        for cell in &cells {
            let real_id = graph.add_cell(cell.clone());
            cell_ids.insert(cell.name.clone(), real_id);
        }
        graph.resolve_dependencies()?;
        let order = graph.topological_order()?;
        Self::print_success(None);

        // Build dependency map
        let deps: HashMap<CellId, Vec<CellId>> = cells
            .iter()
            .map(|cell| {
                let real_id = cell_ids[&cell.name];
                let dep_ids: Vec<CellId> = cell
                    .dependencies
                    .iter()
                    .filter_map(|dep| cell_ids.get(&dep.param_name).copied())
                    .collect();
                (real_id, dep_ids)
            })
            .collect();

        // Build universe
        Self::print_step("Building universe");
        let config = if release {
            CompilerConfig::for_notebook_release(&dirs)
        } else {
            CompilerConfig::for_notebook(&dirs)
        };

        let mut universe_builder = UniverseBuilder::new(config.clone(), toolchain.clone());
        universe_builder.parse_dependencies(&source)?;
        let universe_path = universe_builder.build()?;

        if universe_builder.dependencies().is_empty() {
            Self::print_success(Some("runtime only"));
        } else {
            Self::print_success(None);
        }

        Ok(Self {
            notebook_path: abs_path,
            source,
            dirs,
            toolchain,
            cells,
            cell_ids,
            order,
            deps,
            config,
            universe_builder,
            universe_path,
            release,
        })
    }

    /// Get the notebook name (file stem).
    pub fn notebook_name(&self) -> String {
        self.notebook_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    }

    /// Compile all cells.
    pub fn compile(&self) -> anyhow::Result<CompilationInfo> {
        println!("\n{}Compiling cells...{}", colors::BOLD, colors::RESET);

        let compiler =
            CellCompiler::new(self.config.clone(), self.toolchain.clone())
                .with_universe(self.universe_path.clone());

        let mut compiled_cells = HashMap::new();
        let mut compile_errors = Vec::new();

        for cell in &self.cells {
            print!("  {} {} ... ", colors::DIM, cell.name);
            colors::flush_stdout();

            let real_id = self.cell_ids[&cell.name];
            let deps_hash = self.universe_builder.deps_hash();
            let result = compiler.compile(cell, deps_hash);

            match result {
                CompilationResult::Success(mut compiled) => {
                    let compile_time_ms = compiled.compile_time_ms;
                    compiled.cell_id = real_id;
                    println!(
                        "{}✓{} ({}ms)",
                        colors::GREEN,
                        colors::RESET,
                        compile_time_ms
                    );
                    compiled_cells.insert(
                        real_id,
                        CompiledCellInfo {
                            compiled,
                            dep_count: cell.dependencies.len(),
                            compile_time_ms,
                            cached: false,
                        },
                    );
                }
                CompilationResult::Cached(mut compiled) => {
                    compiled.cell_id = real_id;
                    println!(
                        "{}✓{} {}(cached){}",
                        colors::GREEN,
                        colors::RESET,
                        colors::DIM,
                        colors::RESET
                    );
                    compiled_cells.insert(
                        real_id,
                        CompiledCellInfo {
                            compiled,
                            dep_count: cell.dependencies.len(),
                            compile_time_ms: 0,
                            cached: true,
                        },
                    );
                }
                CompilationResult::Failed { cell_id: _, errors } => {
                    println!("{}✗{}", colors::RED, colors::RESET);
                    for error in &errors {
                        if let Some(rendered) = &error.rendered {
                            eprintln!("{}", rendered);
                        } else {
                            eprintln!("{}", error.format_terminal());
                        }
                    }
                    compile_errors.push((cell.name.clone(), errors));
                }
            }
        }

        Ok(CompilationInfo {
            cells: compiled_cells,
            errors: compile_errors,
        })
    }

    /// Execute cells with the given compilation info.
    ///
    /// If `cell_filter` is provided, only execute that cell and its dependencies.
    pub fn execute(
        &self,
        compilation: &CompilationInfo,
        cell_filter: Option<&str>,
    ) -> anyhow::Result<ExecutionInfo> {
        if !compilation.errors.is_empty() {
            println!(
                "\n{}Compilation failed for {} cell(s){}",
                colors::RED,
                compilation.errors.len(),
                colors::RESET
            );
            anyhow::bail!("Compilation failed");
        }

        println!("\n{}Executing cells...{}", colors::BOLD, colors::RESET);

        let state = StateManager::new(&self.dirs.state_dir)?;
        let mut executor = LinearExecutor::with_state(state);
        executor.set_callback(ProgressCallback::new());

        // Load all compiled cells
        for info in compilation.cells.values() {
            executor.load_cell(info.compiled.clone(), info.dep_count)?;
        }

        // Filter execution order if specific cell requested
        let execution_order = self.filter_execution_order(cell_filter)?;

        // Execute
        let exec_start = Instant::now();
        executor.execute_in_order(&execution_order, &self.deps)?;
        let execution_time = exec_start.elapsed();

        // Collect outputs
        let mut outputs = HashMap::new();
        for &cell_id in &execution_order {
            if let Some(output) = executor.state().get_output(cell_id) {
                outputs.insert(cell_id, output);
            }
        }

        Ok(ExecutionInfo {
            executed_cells: execution_order,
            execution_time,
            outputs,
        })
    }

    /// Execute cells without a callback (for export mode).
    pub fn execute_silent(
        &self,
        compilation: &CompilationInfo,
        cell_filter: Option<&str>,
    ) -> anyhow::Result<ExecutionInfo> {
        if !compilation.errors.is_empty() {
            anyhow::bail!("Compilation failed");
        }

        let state = StateManager::new(&self.dirs.state_dir)?;
        let mut executor = LinearExecutor::with_state(state);

        // Load all compiled cells
        for info in compilation.cells.values() {
            executor.load_cell(info.compiled.clone(), info.dep_count)?;
        }

        // Filter execution order if specific cell requested
        let execution_order = self.filter_execution_order(cell_filter)?;

        // Execute
        let exec_start = Instant::now();
        executor.execute_in_order(&execution_order, &self.deps)?;
        let execution_time = exec_start.elapsed();

        // Collect outputs
        let mut outputs = HashMap::new();
        for &cell_id in &execution_order {
            if let Some(output) = executor.state().get_output(cell_id) {
                outputs.insert(cell_id, output);
            }
        }

        Ok(ExecutionInfo {
            executed_cells: execution_order,
            execution_time,
            outputs,
        })
    }

    /// Filter execution order based on cell filter.
    fn filter_execution_order(&self, cell_filter: Option<&str>) -> anyhow::Result<Vec<CellId>> {
        if let Some(cell_name) = cell_filter {
            let target_id = self
                .cell_ids
                .get(cell_name)
                .ok_or_else(|| anyhow::anyhow!("Cell '{}' not found", cell_name))?;

            Ok(self
                .order
                .iter()
                .copied()
                .filter(|&id| id == *target_id || is_transitive_dependency(id, *target_id, &self.deps))
                .collect())
        } else {
            Ok(self.order.clone())
        }
    }

    /// Get cell info by ID.
    pub fn cell_by_id(&self, cell_id: CellId) -> Option<&CellInfo> {
        self.cells.iter().find(|c| self.cell_ids[&c.name] == cell_id)
    }

    /// Print a setup step.
    fn print_step(name: &str) {
        print!("{}  ◆ {}{} ... ", colors::BLUE, name, colors::RESET);
        colors::flush_stdout();
    }

    /// Print success for a step.
    fn print_success(extra: Option<&str>) {
        match extra {
            Some(s) => println!("{}✓{} ({})", colors::GREEN, colors::RESET, s),
            None => println!("{}✓{}", colors::GREEN, colors::RESET),
        }
    }

    /// Print the header for a run.
    pub fn print_header(&self, action: &str) {
        println!(
            "\n{}Venus{} - {} {}{}{}",
            colors::BOLD,
            colors::RESET,
            action,
            colors::CYAN,
            self.notebook_name(),
            colors::RESET
        );
        println!("{}", "─".repeat(50));
    }
}

/// Check if `dep_id` is a transitive dependency of `target_id`.
pub fn is_transitive_dependency(
    dep_id: CellId,
    target_id: CellId,
    deps: &HashMap<CellId, Vec<CellId>>,
) -> bool {
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![target_id];

    while let Some(current) = stack.pop() {
        if !visited.insert(current) {
            continue;
        }

        if let Some(current_deps) = deps.get(&current) {
            if current_deps.contains(&dep_id) {
                return true;
            }
            stack.extend(current_deps.iter().copied());
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_callback_creation() {
        let callback = ProgressCallback::new();
        assert!(!callback.verbose);

        let verbose = ProgressCallback::verbose();
        assert!(verbose.verbose);
    }
}
