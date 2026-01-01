//! Hot reload support for Venus notebooks.
//!
//! Handles the safe unloading and reloading of cell libraries while
//! preserving state across recompilations.

use std::collections::HashMap;

use crate::compile::{
    CellCompiler, CompilationResult, CompiledCell, CompilerConfig, ToolchainManager,
};
use crate::error::{Error, Result};
use crate::graph::{CellId, CellInfo, GraphEngine};

use super::LinearExecutor;
use super::context::CellContext;

/// Manages hot reloading of cells.
///
/// Coordinates the process of:
/// 1. Saving cell state
/// 2. Unloading the old library
/// 3. Compiling the new version
/// 4. Loading the new library
/// 5. Restoring state (if compatible)
pub struct HotReloader {
    /// Compiler for recompiling cells
    compiler: CellCompiler,
    /// Active cell contexts for cleanup
    contexts: HashMap<CellId, CellContext>,
}

impl HotReloader {
    /// Create a new hot reloader.
    pub fn new(config: CompilerConfig) -> Result<Self> {
        let toolchain = ToolchainManager::new()?;
        let compiler = CellCompiler::new(config, toolchain);

        Ok(Self {
            compiler,
            contexts: HashMap::new(),
        })
    }

    /// Create with an existing compiler.
    pub fn with_compiler(compiler: CellCompiler) -> Self {
        Self {
            compiler,
            contexts: HashMap::new(),
        }
    }

    /// Register a cell context for cleanup tracking.
    pub fn register_context(&mut self, cell_id: CellId, context: CellContext) {
        self.contexts.insert(cell_id, context);
    }

    /// Get a cell's context.
    pub fn get_context(&self, cell_id: CellId) -> Option<&CellContext> {
        self.contexts.get(&cell_id)
    }

    /// Get a mutable reference to a cell's context.
    pub fn get_context_mut(&mut self, cell_id: CellId) -> Option<&mut CellContext> {
        self.contexts.get_mut(&cell_id)
    }

    /// Reload a single cell.
    ///
    /// Returns the new compiled cell on success.
    pub fn reload_cell(
        &mut self,
        executor: &mut LinearExecutor,
        cell_info: &CellInfo,
        deps_hash: u64,
    ) -> Result<CompiledCell> {
        let cell_id = cell_info.id;

        // Step 1: Abort and cleanup the old context
        if let Some(mut ctx) = self.contexts.remove(&cell_id) {
            tracing::info!("Cleaning up cell {:?} for reload", cell_id);
            ctx.abort();
        }

        // Step 2: Save current output (for potential restoration)
        let saved_output = executor.state().get_output(cell_id);

        // Step 3: Unload the old library
        let _old_cell = executor.unload_cell(cell_id);

        // Step 4: Recompile the cell
        tracing::info!("Recompiling cell {:?}", cell_id);
        let result = self.compiler.compile(cell_info, deps_hash);

        match result {
            CompilationResult::Success(compiled) | CompilationResult::Cached(compiled) => {
                // Step 5: Load the new library
                let dep_count = cell_info.dependencies.len();
                executor.load_cell(compiled.clone(), dep_count)?;

                // Step 6: Register new context
                let new_ctx = CellContext::new(cell_id, cell_info.name.clone());
                self.contexts.insert(cell_id, new_ctx);

                // Note: We don't restore the output automatically
                // The caller should decide whether to re-execute or restore
                // based on schema compatibility

                tracing::info!("Cell {:?} reloaded successfully", cell_id);
                Ok(compiled)
            }
            CompilationResult::Failed { cell_id, errors } => {
                // Compilation failed - try to restore the old state
                tracing::error!("Cell {:?} compilation failed: {:?}", cell_id, errors);

                // Restore output if we had one
                if let Some(output) = saved_output {
                    executor
                        .state_mut()
                        .store_output(cell_id, (*output).clone());
                }

                Err(Error::Compilation {
                    cell_id: Some(cell_id.to_string()),
                    message: format!("{} compilation errors", errors.len()),
                })
            }
        }
    }

    /// Reload multiple cells, respecting dependency order.
    ///
    /// Cells are reloaded in topological order to ensure dependencies
    /// are available before dependents are executed.
    pub fn reload_cells(
        &mut self,
        executor: &mut LinearExecutor,
        cells: &[&CellInfo],
        graph: &GraphEngine,
    ) -> Result<Vec<CompiledCell>> {
        // Sort by execution order (topological order)
        let execution_order = graph.topological_order()?;
        let mut sorted_cells: Vec<&CellInfo> = cells.to_vec();
        sorted_cells.sort_by_key(|c| {
            execution_order
                .iter()
                .position(|&id| id == c.id)
                .unwrap_or(usize::MAX)
        });

        // Reload each cell
        let mut compiled = Vec::new();
        for cell in sorted_cells {
            // Calculate deps hash from dependencies
            let deps_hash = self.calculate_deps_hash(cell, graph);
            let result = self.reload_cell(executor, cell, deps_hash)?;
            compiled.push(result);
        }

        Ok(compiled)
    }

    /// Reload a cell and its downstream dependents.
    pub fn reload_cascade(
        &mut self,
        executor: &mut LinearExecutor,
        cell_info: &CellInfo,
        graph: &GraphEngine,
    ) -> Result<Vec<CompiledCell>> {
        let cell_id = cell_info.id;

        // Get all affected cells (the modified cell + its dependents)
        let dependents = graph.invalidated_cells(cell_id);
        let mut affected_ids: Vec<CellId> = vec![cell_id];
        affected_ids.extend(dependents);

        // Invalidate outputs for all affected cells
        executor.state_mut().invalidate_many(&affected_ids);

        // Reload the modified cell
        let deps_hash = self.calculate_deps_hash(cell_info, graph);
        let compiled = self.reload_cell(executor, cell_info, deps_hash)?;

        // Note: Dependents don't need recompilation, just re-execution
        // since their source didn't change

        Ok(vec![compiled])
    }

    /// Calculate dependency hash for a cell.
    fn calculate_deps_hash(&self, cell: &CellInfo, _graph: &GraphEngine) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Hash all dependency cell names (which determines their outputs)
        for dep in &cell.dependencies {
            dep.param_name.hash(&mut hasher);
            dep.param_type.hash(&mut hasher);
        }

        // Could also hash the dependency cells' source hashes for more precision
        // but this is sufficient for most cases

        hasher.finish()
    }

    /// Abort all active cell contexts.
    pub fn abort_all(&mut self) {
        for (cell_id, mut ctx) in self.contexts.drain() {
            tracing::info!("Aborting cell {:?}", cell_id);
            ctx.abort();
        }
    }
}

impl Drop for HotReloader {
    fn drop(&mut self) {
        self.abort_all();
    }
}

/// Result of a hot reload operation.
///
/// TODO(reload): Return from reload_cells with detailed status
#[derive(Debug)]
#[allow(dead_code)]
pub struct ReloadResult {
    /// Cells that were successfully reloaded
    pub reloaded: Vec<CellId>,
    /// Cells that need re-execution (dependents)
    pub needs_execution: Vec<CellId>,
    /// Cells that failed to reload
    pub failed: Vec<(CellId, Error)>,
}

#[allow(dead_code)]
impl ReloadResult {
    /// Create a new empty result.
    pub fn new() -> Self {
        Self {
            reloaded: Vec::new(),
            needs_execution: Vec::new(),
            failed: Vec::new(),
        }
    }

    /// Check if the reload was fully successful.
    pub fn is_success(&self) -> bool {
        self.failed.is_empty()
    }
}

impl Default for ReloadResult {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reload_result() {
        let result = ReloadResult::new();
        assert!(result.is_success());
        assert!(result.reloaded.is_empty());
        assert!(result.needs_execution.is_empty());
    }

    #[test]
    fn test_reload_result_with_failure() {
        let mut result = ReloadResult::new();
        result
            .failed
            .push((CellId::new(0), Error::Execution("test error".to_string())));
        assert!(!result.is_success());
    }
}
