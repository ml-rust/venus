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
                // Note: Returns first error. Error aggregation could be added in future.
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

        // Known limitation: Cells within a level execute sequentially (not in parallel)
        // because execute_cell requires &mut self. This is a correctness-first design.
        // True intra-level parallelism would require:
        //   1. Separating read-only FFI calls from state mutations
        //   2. RwLock instead of Mutex for concurrent reads
        //   3. Read lock during FFI, exclusive lock only for output storage
        // Inter-level parallelism (different levels execute in order) is preserved.

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

// Note: ExecutionStats was removed as unused dead code.
// If metrics collection is needed in the future, it can be re-added
// with fields: cells_executed, levels, max_parallelism, total_time_ms.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execute::AbortHandle;

    #[test]
    fn test_parallel_executor_creation() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ParallelExecutor::new(temp.path()).unwrap();

        // Should be empty initially
        let inner = executor.inner.lock().unwrap();
        assert!(inner.state().stats().cached_outputs == 0);
    }

    #[test]
    fn test_with_state_creation() {
        let temp = tempfile::TempDir::new().unwrap();
        let state = StateManager::new(temp.path()).unwrap();
        let executor = ParallelExecutor::with_state(state);

        let inner = executor.inner.lock().unwrap();
        assert!(inner.state().stats().cached_outputs == 0);
    }

    #[test]
    fn test_set_callback() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut executor = ParallelExecutor::new(temp.path()).unwrap();

        struct TestCallback;
        impl ExecutionCallback for TestCallback {
            fn on_cell_started(&self, _: CellId, _: &str) {}
            fn on_cell_completed(&self, _: CellId, _: &str) {}
            fn on_cell_error(&self, _: CellId, _: &str, _: &Error) {}
            fn on_level_started(&self, _: usize, _: usize) {}
            fn on_level_completed(&self, _: usize) {}
        }

        // Just verify we can set a callback (callback field is private, can't test directly)
        executor.set_callback(TestCallback);
    }

    #[test]
    fn test_abort_handle() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ParallelExecutor::new(temp.path()).unwrap();

        let handle = AbortHandle::new();

        {
            let mut inner = executor.inner.lock().unwrap();
            inner.set_abort_handle(handle.clone());
            assert!(inner.abort_handle().is_some());
        }

        handle.abort();

        // Verify abort is set (will be checked during execution)
        assert!(handle.is_aborted());
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

    #[test]
    fn test_is_loaded() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ParallelExecutor::new(temp.path()).unwrap();

        let cell_id = CellId::new(1);

        let inner = executor.inner.lock().unwrap();
        assert!(!inner.is_loaded(cell_id));
    }

    #[test]
    fn test_get_state_reference() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ParallelExecutor::new(temp.path()).unwrap();

        let inner = executor.inner.lock().unwrap();
        let stats = inner.state().stats();
        assert_eq!(stats.cached_outputs, 0);
    }

    #[test]
    fn test_execute_parallel_aborted() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ParallelExecutor::new(temp.path()).unwrap();

        let handle = AbortHandle::new();
        {
            let mut inner = executor.inner.lock().unwrap();
            inner.set_abort_handle(handle.clone());
        }
        handle.abort();

        let levels: Vec<Vec<CellId>> = vec![vec![CellId::new(1)]];
        let deps = HashMap::new();

        let result = executor.execute_parallel(&levels, &deps);
        assert!(matches!(result, Err(Error::Aborted)));
    }
}
