//! Linear executor for sequential cell execution.
//!
//! Executes cells in dependency order, one at a time.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use libloading::Symbol;

use crate::compile::CompiledCell;
use crate::error::{Error, Result};
use crate::graph::CellId;
use crate::state::{BoxedOutput, StateManager};

use super::context::{AbortHandle, ExecutionCallback};
use super::ffi::{
    EntryFn0, EntryFn1, EntryFn2, EntryFn3, EntryFn4, EntryFn5, EntryFn6, EntryFn7, EntryFn8,
    ExecutionResult, call_cell_n_deps,
};
use super::loaded_cell::LoadedCell;

/// RAII guard for FFI-allocated memory.
/// Ensures libc::free is called even if panic occurs during processing.
struct FfiMemoryGuard {
    ptr: *mut u8,
}

impl FfiMemoryGuard {
    unsafe fn new(ptr: *mut u8) -> Self {
        Self { ptr }
    }

    fn as_slice(&self, len: usize) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, len) }
    }
}

impl Drop for FfiMemoryGuard {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                libc::free(self.ptr as *mut libc::c_void);
            }
        }
    }
}

/// Linear executor that runs cells sequentially in dependency order.
pub struct LinearExecutor {
    /// Loaded cell libraries
    cells: HashMap<CellId, LoadedCell>,
    /// State manager for inputs/outputs
    state: StateManager,
    /// Execution callback for progress reporting
    callback: Option<Box<dyn ExecutionCallback>>,
    /// Abort handle for cooperative cancellation
    abort_handle: Option<AbortHandle>,
}

impl LinearExecutor {
    /// Create a new linear executor.
    pub fn new(state_dir: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            cells: HashMap::new(),
            state: StateManager::new(state_dir)?,
            callback: None,
            abort_handle: None,
        })
    }

    /// Create with an existing state manager.
    pub fn with_state(state: StateManager) -> Self {
        Self {
            cells: HashMap::new(),
            state,
            callback: None,
            abort_handle: None,
        }
    }

    /// Set the execution callback for progress reporting.
    pub fn set_callback(&mut self, callback: impl ExecutionCallback + 'static) {
        self.callback = Some(Box::new(callback));
    }

    /// Set the abort handle for cooperative cancellation.
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

    /// Load a compiled cell for execution.
    pub fn load_cell(&mut self, compiled: CompiledCell, dep_count: usize) -> Result<()> {
        let cell_id = compiled.cell_id;
        let loaded = LoadedCell::load(compiled, dep_count)?;
        self.cells.insert(cell_id, loaded);
        Ok(())
    }

    /// Unload a cell (e.g., before hot-reload).
    pub fn unload_cell(&mut self, cell_id: CellId) -> Option<LoadedCell> {
        self.cells.remove(&cell_id)
    }

    /// Restore a previously unloaded cell (for hot-reload rollback).
    pub fn restore_cell(&mut self, cell: LoadedCell) {
        self.cells.insert(cell.compiled.cell_id, cell);
    }

    /// Check if a cell is loaded.
    pub fn is_loaded(&self, cell_id: CellId) -> bool {
        self.cells.contains_key(&cell_id)
    }

    /// Execute a single cell with the given inputs.
    ///
    /// Returns the serialized output on success.
    /// Returns `Error::Aborted` if abort was requested before execution.
    pub fn execute_cell(
        &mut self,
        cell_id: CellId,
        inputs: &[Arc<BoxedOutput>],
    ) -> Result<BoxedOutput> {
        // Check for abort before starting
        if self.is_aborted() {
            return Err(Error::Aborted);
        }

        let loaded = self
            .cells
            .get(&cell_id)
            .ok_or_else(|| Error::CellNotFound(format!("Cell {:?} not loaded", cell_id)))?;

        // Notify callback
        if let Some(ref callback) = self.callback {
            callback.on_cell_started(cell_id, &loaded.compiled.name);
        }

        // Execute the cell
        let result = self.call_cell_ffi(loaded, inputs);

        // Check for abort after execution (cell may have been aborted mid-flight)
        if self.is_aborted() {
            if let Some(ref callback) = self.callback {
                callback.on_cell_error(cell_id, &loaded.compiled.name, &Error::Aborted);
            }
            return Err(Error::Aborted);
        }

        // Notify callback
        match &result {
            Ok(_) => {
                if let Some(ref callback) = self.callback {
                    callback.on_cell_completed(cell_id, &loaded.compiled.name);
                }
            }
            Err(e) => {
                if let Some(ref callback) = self.callback {
                    callback.on_cell_error(cell_id, &loaded.compiled.name, e);
                }
            }
        }

        result
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
    ///
    /// Returns `Error::Aborted` if abort was requested during execution.
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

    /// Get a reference to the state manager.
    pub fn state(&self) -> &StateManager {
        &self.state
    }

    /// Get a mutable reference to the state manager.
    pub fn state_mut(&mut self) -> &mut StateManager {
        &mut self.state
    }

    /// Call the cell's FFI entry point.
    fn call_cell_ffi(
        &self,
        loaded: &LoadedCell,
        inputs: &[Arc<BoxedOutput>],
    ) -> Result<BoxedOutput> {
        // Verify input count matches
        if inputs.len() != loaded.dep_count {
            return Err(Error::Execution(format!(
                "Cell {} expects {} inputs, got {}",
                loaded.compiled.name,
                loaded.dep_count,
                inputs.len()
            )));
        }

        // For cells with no dependencies, use the simple path
        if loaded.dep_count == 0 {
            return self.call_cell_no_deps(loaded);
        }

        // For cells with dependencies, we need to construct the FFI call dynamically
        // This is complex because the number of parameters varies
        self.call_cell_with_deps(loaded, inputs)
    }

    /// Call a cell with no dependencies.
    fn call_cell_no_deps(&self, loaded: &LoadedCell) -> Result<BoxedOutput> {
        let symbol_name = loaded.entry_symbol();

        // Safety: We trust the symbol exists and has the correct signature
        let func: Symbol<EntryFn0> = unsafe { loaded.library.get(symbol_name.as_bytes()) }
            .map_err(|e| {
                Error::Execution(format!("Failed to get symbol {}: {}", symbol_name, e))
            })?;

        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;

        // Empty widget values (LinearExecutor doesn't support widgets)
        let widget_values: &[u8] = &[];

        // Safety: We're calling a function generated by our compiler
        let result_code = unsafe {
            func(
                widget_values.as_ptr(), widget_values.len(),
                &mut out_ptr, &mut out_len,
            )
        };

        self.process_ffi_result(result_code, out_ptr, out_len, &loaded.compiled.name)
    }

    /// Call a cell with dependencies (up to 8 supported via macro).
    ///
    /// Uses the `call_cell_n_deps!` macro to eliminate code duplication.
    /// Each match arm generates the appropriate typed FFI call.
    fn call_cell_with_deps(
        &self,
        loaded: &LoadedCell,
        inputs: &[Arc<BoxedOutput>],
    ) -> Result<BoxedOutput> {
        let symbol_name = loaded.entry_symbol();

        // Empty widget values (LinearExecutor doesn't support widgets)
        let widget_values: &[u8] = &[];

        // TODO(ffi): Consider using libffi for truly dynamic dispatch
        // to support arbitrary numbers of dependencies without match arms.
        match inputs.len() {
            1 => call_cell_n_deps!(self, loaded, symbol_name, inputs, widget_values, EntryFn1, 0),
            2 => call_cell_n_deps!(self, loaded, symbol_name, inputs, widget_values, EntryFn2, 0, 1),
            3 => call_cell_n_deps!(self, loaded, symbol_name, inputs, widget_values, EntryFn3, 0, 1, 2),
            4 => call_cell_n_deps!(self, loaded, symbol_name, inputs, widget_values, EntryFn4, 0, 1, 2, 3),
            5 => call_cell_n_deps!(self, loaded, symbol_name, inputs, widget_values, EntryFn5, 0, 1, 2, 3, 4),
            6 => call_cell_n_deps!(
                self,
                loaded,
                symbol_name,
                inputs,
                widget_values,
                EntryFn6,
                0,
                1,
                2,
                3,
                4,
                5
            ),
            7 => call_cell_n_deps!(
                self,
                loaded,
                symbol_name,
                inputs,
                widget_values,
                EntryFn7,
                0,
                1,
                2,
                3,
                4,
                5,
                6
            ),
            8 => call_cell_n_deps!(
                self,
                loaded,
                symbol_name,
                inputs,
                widget_values,
                EntryFn8,
                0,
                1,
                2,
                3,
                4,
                5,
                6,
                7
            ),
            n => Err(Error::Execution(format!(
                "Cells with {} dependencies not yet supported (max 8)",
                n
            ))),
        }
    }

    /// Process the FFI result and convert output to BoxedOutput.
    ///
    /// Output format from cells:
    /// - display_len (8 bytes, u64 LE): length of display string
    /// - display_bytes (N bytes): display string (UTF-8)
    /// - widgets_len (8 bytes, u64 LE): length of widgets JSON
    /// - widgets_json (M bytes): JSON-encoded widget definitions
    /// - rkyv_data (remaining bytes): rkyv-serialized data
    pub(crate) fn process_ffi_result(
        &self,
        result_code: i32,
        out_ptr: *mut u8,
        out_len: usize,
        cell_name: &str,
    ) -> Result<BoxedOutput> {
        let result = ExecutionResult::from(result_code);

        match result {
            ExecutionResult::Success => {
                if out_ptr.is_null() || out_len == 0 {
                    return Err(Error::Execution(format!(
                        "Cell {} returned null output",
                        cell_name
                    )));
                }

                // Safety: The cell allocated this memory via libc malloc
                // Use RAII guard to ensure cleanup even if processing panics
                let memory_guard = unsafe { FfiMemoryGuard::new(out_ptr) };
                let bytes = memory_guard.as_slice(out_len).to_vec();
                // Guard's Drop will free the memory automatically

                // Parse output format:
                // display_len (8) | display_bytes (N) | widgets_len (8) | widgets_json (M) | rkyv_data

                if bytes.len() < 16 {
                    return Err(Error::Execution(format!(
                        "Cell {} output too short: {} bytes",
                        cell_name, bytes.len()
                    )));
                }

                // Read display_len
                let display_len_bytes: [u8; 8] = bytes[0..8].try_into().map_err(|_| {
                    Error::Execution(format!(
                        "Cell {} output has malformed display_len field",
                        cell_name
                    ))
                })?;
                let display_len = u64::from_le_bytes(display_len_bytes) as usize;
                let display_end = 8 + display_len;

                if bytes.len() < display_end + 8 {
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
            ExecutionResult::DeserializationError => Err(Error::Execution(format!(
                "Cell {} failed to deserialize input",
                cell_name
            ))),
            ExecutionResult::CellError => Err(Error::Execution(format!(
                "Cell {} returned an error",
                cell_name
            ))),
            ExecutionResult::SerializationError => Err(Error::Execution(format!(
                "Cell {} failed to serialize output",
                cell_name
            ))),
            ExecutionResult::Panic => Err(Error::Execution(format!(
                "Cell {} panicked during execution. Check for unwrap() on None/Err, out-of-bounds access, or other panic sources.",
                cell_name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_executor_creation() {
        let temp = tempfile::TempDir::new().unwrap();
        let executor = LinearExecutor::new(temp.path()).unwrap();
        assert!(executor.cells.is_empty());
    }
}
