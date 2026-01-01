//! Process-based executor for isolated cell execution.
//!
//! Provides true interruption capability by running cells in separate
//! worker processes that can be killed at any time.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::compile::CompiledCell;
use crate::error::{Error, Result};
use crate::graph::CellId;
use crate::ipc::{WorkerKillHandle, WorkerPool};
use crate::state::{BoxedOutput, StateManager};

use super::context::{AbortHandle, ExecutionCallback};

/// Process-based executor that runs cells in isolated worker processes.
///
/// Unlike `LinearExecutor`, this executor can truly interrupt cell execution
/// by killing the worker process. This provides:
/// - Immediate interruption (no need for cooperative checks)
/// - Crash isolation (panics don't affect the server)
/// - Memory isolation (runaway cells can't OOM the server)
pub struct ProcessExecutor {
    /// Compiled cells (we don't load them here, workers do)
    cells: HashMap<CellId, CompiledCellInfo>,
    /// State manager for inputs/outputs
    state: StateManager,
    /// Execution callback for progress reporting
    callback: Option<Box<dyn ExecutionCallback>>,
    /// Abort handle for interruption
    abort_handle: Option<AbortHandle>,
    /// Worker pool for process reuse
    worker_pool: WorkerPool,
    /// Currently executing worker kill handle (thread-safe for external kill).
    /// This is wrapped in Arc<Mutex<>> so it can be cloned and killed from
    /// another thread while execute_cell is running.
    current_worker_kill: Arc<Mutex<Option<WorkerKillHandle>>>,
}

/// Info about a compiled cell (without the loaded library)
struct CompiledCellInfo {
    compiled: CompiledCell,
    dep_count: usize,
}

/// Thread-safe handle for killing an executor's current cell from another thread.
///
/// This can be cloned and passed to another thread, then used to kill
/// whatever cell is currently executing.
#[derive(Clone)]
pub struct ExecutorKillHandle {
    inner: Arc<Mutex<Option<WorkerKillHandle>>>,
}

impl ExecutorKillHandle {
    /// Kill the currently executing cell.
    ///
    /// If no cell is executing, this is a no-op.
    pub fn kill(&self) {
        if let Ok(guard) = self.inner.lock() {
            if let Some(ref kill_handle) = *guard {
                kill_handle.kill();
            }
        }
    }
}

impl ProcessExecutor {
    /// Create a new process executor.
    pub fn new(state_dir: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            cells: HashMap::new(),
            state: StateManager::new(state_dir)?,
            callback: None,
            abort_handle: None,
            worker_pool: WorkerPool::new(4), // Pool of up to 4 workers
            current_worker_kill: Arc::new(Mutex::new(None)),
        })
    }

    /// Create with an existing state manager.
    pub fn with_state(state: StateManager) -> Self {
        Self {
            cells: HashMap::new(),
            state,
            callback: None,
            abort_handle: None,
            worker_pool: WorkerPool::new(4),
            current_worker_kill: Arc::new(Mutex::new(None)),
        }
    }

    /// Create with a pre-warmed worker pool.
    pub fn with_warm_pool(state_dir: impl AsRef<Path>, pool_size: usize) -> Result<Self> {
        Ok(Self {
            cells: HashMap::new(),
            state: StateManager::new(state_dir)?,
            callback: None,
            abort_handle: None,
            worker_pool: WorkerPool::with_warm_workers(pool_size, pool_size.min(2))?,
            current_worker_kill: Arc::new(Mutex::new(None)),
        })
    }

    /// Set the execution callback for progress reporting.
    pub fn set_callback(&mut self, callback: impl ExecutionCallback + 'static) {
        self.callback = Some(Box::new(callback));
    }

    /// Set the abort handle for interruption.
    pub fn set_abort_handle(&mut self, handle: AbortHandle) {
        self.abort_handle = Some(handle);
    }

    /// Get the current abort handle.
    pub fn abort_handle(&self) -> Option<&AbortHandle> {
        self.abort_handle.as_ref()
    }

    /// Check if execution has been aborted.
    fn is_aborted(&self) -> bool {
        self.abort_handle
            .as_ref()
            .is_some_and(|h| h.is_aborted())
    }

    /// Register a compiled cell for execution.
    ///
    /// Unlike `LinearExecutor::load_cell`, this doesn't actually load the dylib.
    /// The worker process will load it when executing.
    pub fn register_cell(&mut self, compiled: CompiledCell, dep_count: usize) {
        let cell_id = compiled.cell_id;
        self.cells.insert(cell_id, CompiledCellInfo {
            compiled,
            dep_count,
        });
    }

    /// Unregister a cell.
    pub fn unregister_cell(&mut self, cell_id: CellId) -> Option<CompiledCell> {
        self.cells.remove(&cell_id).map(|info| info.compiled)
    }

    /// Check if a cell is registered.
    pub fn is_registered(&self, cell_id: CellId) -> bool {
        self.cells.contains_key(&cell_id)
    }

    /// Execute a single cell with the given inputs.
    ///
    /// This runs the cell in a worker process that can be killed for interruption.
    pub fn execute_cell(
        &mut self,
        cell_id: CellId,
        inputs: &[Arc<BoxedOutput>],
    ) -> Result<BoxedOutput> {
        self.execute_cell_with_widgets(cell_id, inputs, Vec::new())
            .map(|(output, _widgets_json)| output)
    }

    /// Execute a single cell with the given inputs and widget values.
    ///
    /// This runs the cell in a worker process that can be killed for interruption.
    /// Returns the cell output and any registered widget definitions as JSON.
    pub fn execute_cell_with_widgets(
        &mut self,
        cell_id: CellId,
        inputs: &[Arc<BoxedOutput>],
        widget_values_json: Vec<u8>,
    ) -> Result<(BoxedOutput, Vec<u8>)> {
        // Check for abort before starting
        if self.is_aborted() {
            return Err(Error::Aborted);
        }

        let info = self
            .cells
            .get(&cell_id)
            .ok_or_else(|| Error::CellNotFound(format!("Cell {:?} not registered", cell_id)))?;

        let compiled = &info.compiled;
        let dep_count = info.dep_count;

        // Notify callback
        if let Some(ref callback) = self.callback {
            callback.on_cell_started(cell_id, &compiled.name);
        }

        // Get a worker from the pool
        let mut worker = self.worker_pool.get()?;

        // Store kill handle for potential interruption (thread-safe)
        {
            let mut kill_guard = self.current_worker_kill.lock().unwrap();
            *kill_guard = Some(WorkerKillHandle::new(&worker));
        }

        // Load the cell in the worker
        worker.load_cell(
            compiled.dylib_path.clone(),
            dep_count,
            compiled.entry_symbol.clone(),
            compiled.name.clone(),
        )?;

        // Prepare inputs as raw bytes
        let input_bytes: Vec<Vec<u8>> = inputs
            .iter()
            .map(|output| output.bytes().to_vec())
            .collect();

        // Check for abort after load
        if self.is_aborted() {
            // Kill the worker and return abort error
            let _ = worker.kill();
            {
                let mut kill_guard = self.current_worker_kill.lock().unwrap();
                *kill_guard = None;
            }
            if let Some(ref callback) = self.callback {
                callback.on_cell_error(cell_id, &compiled.name, &Error::Aborted);
            }
            return Err(Error::Aborted);
        }

        // Execute the cell with widget values
        let result = worker.execute_with_widgets(input_bytes, widget_values_json);

        // Clear kill handle
        {
            let mut kill_guard = self.current_worker_kill.lock().unwrap();
            *kill_guard = None;
        }

        // Return worker to pool (if still alive)
        self.worker_pool.put(worker);

        // Check for abort after execution
        if self.is_aborted() {
            if let Some(ref callback) = self.callback {
                callback.on_cell_error(cell_id, &compiled.name, &Error::Aborted);
            }
            return Err(Error::Aborted);
        }

        // Process result
        match result {
            Ok((bytes, widgets_json)) => {
                // Parse the output bytes into BoxedOutput
                let output = self.parse_output_bytes(&bytes, &compiled.name)?;

                if let Some(ref callback) = self.callback {
                    callback.on_cell_completed(cell_id, &compiled.name);
                }

                Ok((output, widgets_json))
            }
            Err(e) => {
                if let Some(ref callback) = self.callback {
                    callback.on_cell_error(cell_id, &compiled.name, &e);
                }
                Err(e)
            }
        }
    }

    /// Parse output bytes from worker into BoxedOutput.
    ///
    /// Output format from cells:
    /// - display_len (8 bytes, u64 LE): length of display string
    /// - display_bytes (N bytes): display string (UTF-8)
    /// - widgets_len (8 bytes, u64 LE): length of widgets JSON
    /// - widgets_json (M bytes): JSON-encoded widget definitions
    /// - rkyv_data (remaining bytes): rkyv-serialized data
    fn parse_output_bytes(&self, bytes: &[u8], cell_name: &str) -> Result<BoxedOutput> {
        if bytes.len() < 16 {
            return Err(Error::Execution(format!(
                "Cell {} output too short: {} bytes",
                cell_name,
                bytes.len()
            )));
        }

        // Read display_len
        let display_len = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
        let display_end = 8 + display_len;

        if bytes.len() < display_end {
            return Err(Error::Execution(format!(
                "Cell {} output too short for display data",
                cell_name
            )));
        }

        // Worker already stripped widgets_len and widgets_json
        // Format is: display_len | display_bytes | rkyv_data
        let display_text = String::from_utf8_lossy(&bytes[8..display_end]).to_string();
        let rkyv_data = bytes[display_end..].to_vec();

        Ok(BoxedOutput::from_raw_bytes_with_display(rkyv_data, display_text))
    }

    /// Execute a cell and store the output in the state manager.
    pub fn execute_and_store(
        &mut self,
        cell_id: CellId,
        inputs: &[Arc<BoxedOutput>],
    ) -> Result<()> {
        let output = self.execute_cell(cell_id, inputs)?;
        self.state.store_output(cell_id, output);
        Ok(())
    }

    /// Execute cells in the given order, resolving dependencies from state.
    pub fn execute_in_order(
        &mut self,
        order: &[CellId],
        deps: &HashMap<CellId, Vec<CellId>>,
    ) -> Result<()> {
        for &cell_id in order {
            // Check for abort before each cell
            if self.is_aborted() {
                return Err(Error::Aborted);
            }

            // Gather inputs from dependencies
            let dep_ids = deps.get(&cell_id).cloned().unwrap_or_default();
            let inputs: Vec<Arc<BoxedOutput>> = dep_ids
                .iter()
                .filter_map(|&dep_id| self.state.get_output(dep_id))
                .collect();

            // Check we have all required inputs
            if inputs.len() != dep_ids.len() {
                return Err(Error::Execution(format!(
                    "Missing dependencies for cell {:?}: expected {}, got {}",
                    cell_id,
                    dep_ids.len(),
                    inputs.len()
                )));
            }

            self.execute_and_store(cell_id, &inputs)?;
        }

        Ok(())
    }

    /// Kill the currently executing cell immediately.
    ///
    /// This is the key feature - we can terminate the worker process
    /// mid-computation without any cooperation from the cell.
    /// This method is thread-safe and can be called from any thread.
    pub fn kill_current(&self) {
        if let Ok(guard) = self.current_worker_kill.lock() {
            if let Some(ref kill_handle) = *guard {
                kill_handle.kill();
            }
        }
    }

    /// Get a handle that can be used to kill the current execution from another thread.
    ///
    /// Returns `None` if no execution is in progress.
    /// The returned handle is safe to clone and use from any thread.
    pub fn get_kill_handle(&self) -> Option<ExecutorKillHandle> {
        Some(ExecutorKillHandle {
            inner: self.current_worker_kill.clone(),
        })
    }

    /// Abort execution and kill any running cell.
    ///
    /// Sets the abort flag and kills the current worker.
    pub fn abort(&mut self) {
        if let Some(ref handle) = self.abort_handle {
            handle.abort();
        }
        self.kill_current();
    }

    /// Get a reference to the state manager.
    pub fn state(&self) -> &StateManager {
        &self.state
    }

    /// Get a mutable reference to the state manager.
    pub fn state_mut(&mut self) -> &mut StateManager {
        &mut self.state
    }

    /// Shutdown the executor and all workers.
    pub fn shutdown(&mut self) {
        self.worker_pool.shutdown();
    }
}

impl Drop for ProcessExecutor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_executor_creation() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ProcessExecutor::new(temp.path()).unwrap();
        assert!(executor.cells.is_empty());
    }

    #[test]
    #[ignore = "Requires venus-worker binary"]
    fn test_process_executor_worker_pool() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = ProcessExecutor::with_warm_pool(temp.path(), 2).unwrap();
        assert_eq!(executor.worker_pool.available_count(), 2);
    }
}
