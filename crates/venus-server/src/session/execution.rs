use super::NotebookSession;
use crate::error::{ServerError, ServerResult};
use crate::protocol::{CellOutput, CellStatus, ServerMessage};
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};
use venus_core::compile::{CellCompiler, CompilationResult};
use venus_core::execute::{ExecutorKillHandle, ProcessExecutor};
use venus_core::graph::CellId;
use venus_core::paths::NotebookDirs;
use venus_core::state::BoxedOutput;
use venus_core::widgets::WidgetDef;

impl NotebookSession {
    /// Execute a specific cell.
    ///
    /// Uses process isolation - the cell runs in a worker process that can
    /// be killed immediately for interruption.
    pub async fn execute_cell(&mut self, cell_id: CellId) -> ServerResult<()> {
        // Get cell name before potential reload (IDs change after reload!)
        let cell_name = self
            .get_cell(cell_id)
            .map(|c| c.name.clone())
            .ok_or(ServerError::CellNotFound(cell_id))?;

        // Save pending edit to disk before executing
        if let Some(new_source) = self.pending_edits.remove(&cell_id) {
            self.edit_cell(cell_id, new_source)?;
        }
        if self.executing {
            return Err(ServerError::ExecutionInProgress);
        }

        // After reload(), cell IDs change! Find cell by name instead
        let cell = self
            .cells
            .iter()
            .find(|c| c.name == cell_name)
            .ok_or(ServerError::CellNotFound(cell_id))?
            .clone();

        let cell_id = cell.id; // Use the NEW ID after reload

        self.executing = true;

        // Reset interrupted flag at the start of each execution
        self.interrupted.store(false, Ordering::SeqCst);

        // Check if all dependencies have outputs available
        let missing_deps: Vec<&str> = cell
            .dependencies
            .iter()
            .filter(|dep| {
                let producer = self.cells.iter().find(|c| c.name == dep.param_name);
                match producer {
                    Some(c) => !self.cell_outputs.contains_key(&c.id),
                    None => true,
                }
            })
            .map(|d| d.param_name.as_str())
            .collect();

        if !missing_deps.is_empty() {
            self.set_cell_status(cell_id, CellStatus::Error);
            self.broadcast(ServerMessage::CellError {
                cell_id,
                error: format!(
                    "Missing dependencies: {}. Run dependent cells first.",
                    missing_deps.join(", ")
                ),
                location: None,
            });
            self.executing = false;
            return Ok(());
        }

        // Compile
        self.set_cell_status(cell_id, CellStatus::Compiling);

        let mut compiler = CellCompiler::new(self.config.clone(), self.toolchain.clone());
        if let Some(ref up) = self.universe_path {
            compiler = compiler.with_universe(up.clone());
        }

        let result = compiler.compile(&cell, self.deps_hash);

        match result {
            CompilationResult::Success(compiled) | CompilationResult::Cached(compiled) => {
                // Execute
                self.set_cell_status(cell_id, CellStatus::Running);
                self.broadcast(ServerMessage::CellStarted { cell_id });

                let start = Instant::now();

                // Register the compiled cell with the executor
                self.executor
                    .register_cell(compiled, cell.dependencies.len());

                // Gather dependency outputs in the order the cell expects them
                let inputs: Vec<Arc<BoxedOutput>> = cell
                    .dependencies
                    .iter()
                    .filter_map(|dep| {
                        self.cells
                            .iter()
                            .find(|c| c.name == dep.param_name)
                            .and_then(|c| self.cell_outputs.get(&c.id).cloned())
                    })
                    .collect();

                // Get ALL widget values from all cells (widgets can be in any cell)
                let widget_values = self.get_all_widget_values();
                let widget_values_json = if widget_values.is_empty() {
                    Vec::new()
                } else {
                    serde_json::to_vec(&widget_values).unwrap_or_default()
                };

                // Execute the cell in an isolated worker process with widget values
                let exec_result =
                    self.executor
                        .execute_cell_with_widgets(cell_id, &inputs, widget_values_json);

                let duration = start.elapsed();

                match exec_result {
                    Ok((output, widgets_json)) => {
                        // Check if output changed (for smart dirty marking)
                        let old_hash = self
                            .cell_outputs
                            .get(&cell_id)
                            .map(|old| Self::output_hash(old));
                        let new_hash = Self::output_hash(&output);
                        let output_changed = old_hash.is_none_or(|h| h != new_hash);

                        // Store output for dependent cells
                        let output_arc = Arc::new(output);
                        self.cell_outputs.insert(cell_id, output_arc.clone());

                        // Also store in executor state for consistency
                        self.executor
                            .state_mut()
                            .store_output(cell_id, (*output_arc).clone());

                        // Parse and store widget definitions
                        let widgets: Vec<WidgetDef> = if widgets_json.is_empty() {
                            Vec::new()
                        } else {
                            serde_json::from_slice(&widgets_json).unwrap_or_default()
                        };
                        self.store_widget_defs(cell_id, widgets.clone());

                        let cell_output = CellOutput {
                            text: output_arc.display_text().map(|s| s.to_string()),
                            html: None,
                            image: None,
                            json: None,
                            widgets,
                        };

                        // Add to history
                        self.add_to_history(cell_id, output_arc.clone(), cell_output.clone());

                        if let Some(state) = self.cell_states.get_mut(&cell_id) {
                            state.set_status(CellStatus::Success);
                            state.set_output(Some(cell_output.clone()));
                            state.set_dirty(false);
                        }

                        // Mark dependents dirty if output changed
                        if output_changed {
                            let dirty_cells = self.mark_dependents_dirty_and_get(cell_id);
                            for dirty_id in dirty_cells {
                                self.broadcast(ServerMessage::CellDirty { cell_id: dirty_id });
                            }
                        }

                        self.broadcast(ServerMessage::CellCompleted {
                            cell_id,
                            duration_ms: duration.as_millis() as u64,
                            output: Some(cell_output),
                        });
                    }
                    Err(e) => {
                        // Check if this was an abort or user-initiated interrupt
                        let was_interrupted = self.interrupted.swap(false, Ordering::SeqCst);
                        if matches!(e, venus_core::Error::Aborted) || was_interrupted {
                            // Send friendly "interrupted" message instead of error
                            self.set_cell_status(cell_id, CellStatus::Idle);
                            self.broadcast(ServerMessage::ExecutionAborted {
                                cell_id: Some(cell_id),
                            });
                        } else {
                            self.set_cell_status(cell_id, CellStatus::Error);
                            self.broadcast(ServerMessage::CellError {
                                cell_id,
                                error: e.to_string(),
                                location: None,
                            });
                        }
                    }
                }
            }
            CompilationResult::Failed { errors, .. } => {
                self.set_cell_status(cell_id, CellStatus::Error);

                let compile_errors = errors
                    .iter()
                    .map(|e| crate::protocol::CompileErrorInfo {
                        message: e.message.clone(),
                        code: e.code.clone(),
                        location: e.spans.first().map(|s| crate::protocol::SourceLocation {
                            line: s.location.line as u32,
                            column: s.location.column as u32,
                            end_line: s.end_location.as_ref().map(|l| l.line as u32),
                            end_column: s.end_location.as_ref().map(|l| l.column as u32),
                        }),
                        rendered: e.rendered.clone(),
                    })
                    .collect();

                self.broadcast(ServerMessage::CompileError {
                    cell_id,
                    errors: compile_errors,
                });
            }
        }

        self.executing = false;
        Ok(())
    }

    /// Execute all cells in order.
    ///
    /// If `execution_timeout` is set, kills the worker process after that duration.
    /// Unlike cooperative cancellation, this immediately terminates the cell.
    pub async fn execute_all(&mut self) -> ServerResult<()> {
        let order = self.graph.topological_order()?;
        let start = Instant::now();
        let timeout = self.execution_timeout;

        for cell_id in order {
            // Check timeout before each cell
            if timeout.is_some_and(|max_duration| start.elapsed() > max_duration) {
                self.executor.abort();
                self.broadcast(ServerMessage::ExecutionAborted {
                    cell_id: Some(cell_id),
                });
                return Err(ServerError::ExecutionTimeout);
            }

            self.execute_cell(cell_id).await?;
        }
        Ok(())
    }

    /// Check if execution is in progress.
    pub fn is_executing(&self) -> bool {
        self.executing
    }

    /// Abort the current execution immediately.
    ///
    /// Unlike cooperative cancellation, this **kills the worker process**,
    /// providing true interruption even for long-running computations.
    /// Returns `true` if there was an execution in progress to abort.
    pub fn abort(&mut self) -> bool {
        if self.executing {
            // Kill the worker process - this is immediate
            self.executor.abort();
            self.broadcast(ServerMessage::ExecutionAborted { cell_id: None });
            self.executing = false;
            true
        } else {
            false
        }
    }

    /// Set the execution timeout for execute_all.
    ///
    /// When set, execute_all will kill the worker process after this duration,
    /// providing immediate interruption of even long-running cells.
    pub fn set_execution_timeout(&mut self, timeout: Option<Duration>) {
        self.execution_timeout = timeout;
    }

    /// Get the current execution timeout.
    pub fn execution_timeout(&self) -> Option<Duration> {
        self.execution_timeout
    }

    /// Set the interrupted flag.
    ///
    /// When true, execution errors will be reported as "interrupted"
    /// rather than as failures, showing a friendly message to users.
    pub fn set_interrupted(&mut self, value: bool) {
        self.interrupted.store(value, Ordering::SeqCst);
    }

    /// Get a kill handle for the executor.
    ///
    /// This handle can be used from another task to kill the current execution
    /// without needing to acquire the session lock.
    pub fn get_kill_handle(&self) -> Option<ExecutorKillHandle> {
        self.executor.get_kill_handle()
    }

    /// Restart the kernel: kill WorkerPool, spin up new one, clear memory state, preserve source.
    ///
    /// This clears all execution state including:
    /// - Cell outputs and output history
    /// - Widget values
    /// - Cached serialized outputs
    /// - Cell execution status (all cells reset to Idle)
    ///
    /// Source code and cell definitions are preserved.
    pub fn restart_kernel(&mut self) -> ServerResult<()> {
        // Abort any running execution first
        if self.executing {
            self.abort();
        }

        // Reload notebook from disk (picks up any file changes)
        self.reload()?;

        // Shutdown old executor and worker pool
        self.executor.shutdown();

        // Reconstruct state directory path
        let dirs = NotebookDirs::from_notebook_path(&self.path)?;

        // Create new ProcessExecutor with warm worker pool
        self.executor = ProcessExecutor::new(&dirs.state_dir)?;

        // Clear all execution state
        self.cell_outputs.clear();
        self.widget_values.clear();
        self.widget_defs.clear();
        self.cell_output_history.clear();
        self.cell_history_index.clear();

        // Reset all cell states to Idle and clear outputs
        for state in self.cell_states.values_mut() {
            state.set_status(CellStatus::Idle);
            state.clear_output();
            state.set_dirty(false);
        }

        // Broadcast kernel restarted message
        self.broadcast(ServerMessage::KernelRestarted { error: None });

        // Send updated state to all clients
        let state_msg = self.get_state();
        self.broadcast(state_msg);

        Ok(())
    }
}
