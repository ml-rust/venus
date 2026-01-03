//! Parallel executor for Venus notebooks.
//!
//! Executes cells in parallel based on dependency levels using Rayon.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rayon::prelude::*;

use super::context::ExecutionCallback;
use super::{LinearExecutor, LoadedCell};
use crate::compile::CompiledCell;
use crate::error::{Error, Result};
use crate::graph::CellId;
use crate::state::{BoxedOutput, StateManager};

/// Parallel executor that runs independent cells concurrently.
///
/// Cells are grouped by dependency level and executed in parallel
/// within each level. Levels are processed sequentially to maintain
/// dependency ordering.
pub struct ParallelExecutor {
    /// Inner linear executor (wrapped for thread-safe access)
    inner: Arc<Mutex<LinearExecutor>>,
    /// Execution callback
    callback: Option<Arc<dyn ExecutionCallback>>,
}

/// Helper to convert PoisonError to our Error type.
///
/// Centralizes lock error handling to eliminate duplication.
fn lock_error<T>(e: PoisonError<T>) -> Error {
    Error::Execution(format!("Executor lock poisoned (thread panicked): {}", e))
}

impl ParallelExecutor {
    /// Create a new parallel executor.
    pub fn new(state_dir: impl AsRef<Path>) -> Result<Self> {
        let inner = LinearExecutor::new(state_dir)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
            callback: None,
        })
    }

    /// Create with an existing state manager.
    pub fn with_state(state: StateManager) -> Self {
        let inner = LinearExecutor::with_state(state);
        Self {
            inner: Arc::new(Mutex::new(inner)),
            callback: None,
        }
    }

    /// Acquire the inner executor lock.
    ///
    /// Helper method to centralize lock acquisition and error handling.
    fn acquire_lock(&self) -> Result<MutexGuard<'_, LinearExecutor>> {
        self.inner.lock().map_err(lock_error)
    }

    /// Set the execution callback.
    pub fn set_callback(&mut self, callback: impl ExecutionCallback + 'static) {
        self.callback = Some(Arc::new(callback));
    }

    /// Load a compiled cell for execution.
    pub fn load_cell(&self, compiled: CompiledCell, dep_count: usize) -> Result<()> {
        self.acquire_lock()?.load_cell(compiled, dep_count)
    }

    /// Unload a cell.
    pub fn unload_cell(&self, cell_id: CellId) -> Result<Option<LoadedCell>> {
        Ok(self.acquire_lock()?.unload_cell(cell_id))
    }

    /// Execute cells in parallel based on dependency levels.
    ///
    /// # Arguments
    /// * `levels` - Cells grouped by dependency level (earlier levels have no deps on later ones)
    /// * `deps` - Dependency map: cell_id -> list of dependency cell_ids
    pub fn execute_parallel(
        &self,
        levels: &[Vec<CellId>],
        deps: &HashMap<CellId, Vec<CellId>>,
    ) -> Result<()> {
        for (level_idx, level_cells) in levels.iter().enumerate() {
            if level_cells.is_empty() {
                continue;
            }

            // Notify callback
            if let Some(ref callback) = self.callback {
                callback.on_level_started(level_idx, level_cells.len());
            }

            // Execute all cells in this level in parallel
            let results: Vec<Result<()>> = level_cells
                .par_iter()
                .map(|&cell_id| self.execute_single_cell(cell_id, deps))
                .collect();

            // Check for errors
            let errors: Vec<_> = results.into_iter().filter_map(|r| r.err()).collect();
            if !errors.is_empty() {
                // TODO(errors): Aggregate multiple errors instead of returning first
                return Err(errors.into_iter().next().unwrap());
            }

            // Notify callback
            if let Some(ref callback) = self.callback {
                callback.on_level_completed(level_idx);
            }
        }

        Ok(())
    }

    /// Execute a single cell, gathering its dependencies.
    ///
    /// Acquires the lock once for the entire operation to minimize contention.
    fn execute_single_cell(
        &self,
        cell_id: CellId,
        deps: &HashMap<CellId, Vec<CellId>>,
    ) -> Result<()> {
        let dep_ids = deps.get(&cell_id).cloned().unwrap_or_default();

        // TODO(parallelism): Current implementation serializes execution within a level
        // because execute_cell requires &mut self. To enable true parallelism:
        // 1. Refactor LinearExecutor to separate read-only operations (loading cells, calling FFI)
        //    from state mutations (storing outputs)
        // 2. Use RwLock instead of Mutex to allow concurrent reads
        // 3. Hold only read lock during FFI execution, exclusive lock only for output storage
        //
        // For now, we prioritize correctness over parallelism within a level.
        // Inter-level parallelism is preserved.

        let mut inner = self.acquire_lock()?;

        // Gather dependency outputs
        let inputs: Vec<Arc<BoxedOutput>> = dep_ids
            .iter()
            .filter_map(|&dep_id| inner.state().get_output(dep_id))
            .collect();

        // Verify we have all inputs
        if inputs.len() != dep_ids.len() {
            return Err(Error::Execution(format!(
                "Missing dependencies for cell {:?}: expected {}, got {}",
                cell_id,
                dep_ids.len(),
                inputs.len()
            )));
        }

        // Execute and store output atomically to prevent races
        let output = inner.execute_cell(cell_id, &inputs)?;
        inner.state_mut().store_output(cell_id, output);

        Ok(())
    }

    /// Get access to the inner executor.
    pub fn inner(&self) -> &Arc<Mutex<LinearExecutor>> {
        &self.inner
    }

    /// Flush all cached outputs to disk.
    pub fn flush(&self) -> Result<()> {
        self.acquire_lock()?.state_mut().flush()
    }
}

/// Statistics about parallel execution.
///
/// TODO(metrics): Populate and return from execute_parallel
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct ExecutionStats {
    /// Total cells executed
    pub cells_executed: usize,
    /// Number of parallel levels
    pub levels: usize,
    /// Maximum parallelism achieved
    pub max_parallelism: usize,
    /// Total execution time in milliseconds
    pub total_time_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_executor_creation() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ParallelExecutor::new(temp.path()).unwrap();

        // Should be empty initially
        let inner = executor.inner.lock().unwrap();
        assert!(inner.state().stats().cached_outputs == 0);
    }

    #[test]
    fn test_empty_levels() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ParallelExecutor::new(temp.path()).unwrap();

        let levels: Vec<Vec<CellId>> = vec![];
        let deps = HashMap::new();

        // Should succeed with no work to do
        executor.execute_parallel(&levels, &deps).unwrap();
    }
}
