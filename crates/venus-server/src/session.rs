//! Notebook session management.
//!
//! Manages the state of an active notebook session including
//! compilation, execution, and output caching.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::{RwLock, broadcast};
use venus::widgets::{WidgetDef, WidgetValue};
use venus_core::compile::{
    CellCompiler, CompilationResult, CompilerConfig, ToolchainManager, UniverseBuilder,
};
use venus_core::execute::{ExecutorKillHandle, ProcessExecutor};
use venus_core::graph::{CellId, CellInfo, CellParser, GraphEngine};
use venus_core::paths::NotebookDirs;

use crate::error::{ServerError, ServerResult};
use crate::protocol::{CellOutput, CellState, CellStatus, DependencyEdge, ServerMessage};
use venus_core::state::BoxedOutput;

/// Shared interrupt flag that can be checked without locks.
pub type InterruptFlag = Arc<AtomicBool>;

/// Capacity for the broadcast channel.
/// 256 messages should be sufficient for normal notebook operation.
/// If clients fall behind, older messages will be dropped.
const MESSAGE_CHANNEL_CAPACITY: usize = 256;

/// A notebook session.
pub struct NotebookSession {
    /// Path to the notebook file.
    path: PathBuf,

    /// Parsed cells.
    cells: Vec<CellInfo>,

    /// Dependency graph.
    graph: GraphEngine,

    /// Cell states for clients.
    cell_states: HashMap<CellId, CellState>,

    /// Toolchain manager.
    toolchain: ToolchainManager,

    /// Compiler configuration.
    config: CompilerConfig,

    /// Universe path (compiled dependencies).
    universe_path: Option<PathBuf>,

    /// Dependencies hash for cache invalidation.
    deps_hash: u64,

    /// Broadcast channel for server messages.
    tx: broadcast::Sender<ServerMessage>,

    /// Whether an execution is in progress.
    executing: bool,

    /// Cached cell outputs for dependency passing.
    /// Maps cell ID to its serialized output.
    cell_outputs: HashMap<CellId, Arc<BoxedOutput>>,

    /// Process-based executor for isolated cell execution.
    /// Uses worker processes that can be killed for true interruption.
    executor: ProcessExecutor,

    /// Optional execution timeout for execute_all.
    /// After this duration, the executor kills the current worker.
    execution_timeout: Option<Duration>,

    /// Shared flag indicating if current execution was interrupted by user.
    /// When true, errors should be reported as "interrupted" not as failures.
    /// This is shared with AppState so interrupt handler can set it.
    interrupted: InterruptFlag,

    /// Widget values per cell.
    /// Maps cell ID -> widget ID -> current value.
    widget_values: HashMap<CellId, HashMap<String, WidgetValue>>,

    /// Widget definitions per cell (from last execution).
    /// Used to send widget state to newly connected clients.
    widget_defs: HashMap<CellId, Vec<WidgetDef>>,

    /// Execution history per cell.
    /// Stores both serialized output (for dependent cells) and display output.
    cell_output_history: HashMap<CellId, Vec<OutputHistoryEntry>>,

    /// Current history index per cell.
    cell_history_index: HashMap<CellId, usize>,
}

/// Maximum number of history entries per cell.
const MAX_HISTORY_PER_CELL: usize = 10;

/// A single history entry for a cell's execution.
#[derive(Clone)]
pub struct OutputHistoryEntry {
    /// Serialized output for passing to dependent cells.
    pub serialized: Arc<BoxedOutput>,
    /// Display output for the frontend.
    pub display: CellOutput,
    /// Timestamp when this execution completed.
    pub timestamp: u64,
}

/// Thread-safe session handle.
pub type SessionHandle = Arc<RwLock<NotebookSession>>;

impl NotebookSession {
    /// Create a new notebook session.
    ///
    /// Uses process isolation for cell execution, allowing true interruption
    /// by killing worker processes.
    ///
    /// The `interrupted` flag is shared with AppState so the interrupt handler
    /// can set it without needing the session lock.
    pub fn new(
        path: impl AsRef<Path>,
        interrupted: InterruptFlag,
    ) -> ServerResult<(Self, broadcast::Receiver<ServerMessage>)> {
        let path = path.as_ref().canonicalize().map_err(|e| ServerError::Io {
            path: path.as_ref().to_path_buf(),
            message: e.to_string(),
        })?;

        // Set up directories using shared abstraction
        let dirs = NotebookDirs::from_notebook_path(&path)?;

        let toolchain = ToolchainManager::new()?;
        let config = CompilerConfig::for_notebook(&dirs);

        let (tx, rx) = broadcast::channel(MESSAGE_CHANNEL_CAPACITY);

        // Create process executor with warm worker pool
        let executor = ProcessExecutor::new(&dirs.state_dir)?;

        let mut session = Self {
            path,
            cells: Vec::new(),
            graph: GraphEngine::new(),
            cell_states: HashMap::new(),
            toolchain,
            config,
            universe_path: None,
            deps_hash: 0,
            tx,
            executing: false,
            cell_outputs: HashMap::new(),
            executor,
            execution_timeout: None,
            interrupted,
            widget_values: HashMap::new(),
            widget_defs: HashMap::new(),
            cell_output_history: HashMap::new(),
            cell_history_index: HashMap::new(),
        };

        session.reload()?;

        Ok((session, rx))
    }

    /// Get the notebook path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Subscribe to server messages.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.tx.subscribe()
    }

    /// Get a cell by ID.
    fn get_cell(&self, cell_id: CellId) -> Option<&CellInfo> {
        self.cells.iter().find(|c| c.id == cell_id)
    }

    /// Set the status of a cell.
    fn set_cell_status(&mut self, cell_id: CellId, status: CellStatus) {
        if let Some(state) = self.cell_states.get_mut(&cell_id) {
            state.status = status;
        }
    }

    /// Broadcast a server message, ignoring send failures.
    pub fn broadcast(&self, msg: ServerMessage) {
        let _ = self.tx.send(msg);
    }

    /// Reload the notebook from disk.
    pub fn reload(&mut self) -> ServerResult<()> {
        let source = std::fs::read_to_string(&self.path)?;

        // Parse cells
        let mut parser = CellParser::new();
        self.cells = parser.parse_file(&self.path)?;

        // Build graph and update cells with real IDs (parser returns placeholder IDs)
        self.graph = GraphEngine::new();
        for cell in &mut self.cells {
            let real_id = self.graph.add_cell(cell.clone());
            cell.id = real_id;
        }
        self.graph.resolve_dependencies()?;

        // Build universe (always needed for bincode/serde runtime)
        let mut universe_builder =
            UniverseBuilder::new(self.config.clone(), self.toolchain.clone());
        universe_builder.parse_dependencies(&source)?;

        self.universe_path = Some(universe_builder.build()?);
        self.deps_hash = universe_builder.deps_hash();

        // Update cell states
        self.update_cell_states();

        // Broadcast graph update
        self.broadcast_graph_update();

        Ok(())
    }

    /// Update cell states from parsed cells.
    fn update_cell_states(&mut self) {
        let mut new_states = HashMap::new();

        for cell in &self.cells {
            let existing = self.cell_states.get(&cell.id);
            let state = CellState {
                id: cell.id,
                name: cell.name.clone(),
                source: cell.source_code.clone(),
                description: cell.doc_comment.clone(),
                return_type: cell.return_type.clone(),
                dependencies: cell
                    .dependencies
                    .iter()
                    .map(|d| d.param_name.clone())
                    .collect(),
                status: existing.map(|s| s.status).unwrap_or_default(),
                output: existing.and_then(|s| s.output.clone()),
                dirty: existing.map(|s| s.dirty).unwrap_or(true),
            };
            new_states.insert(cell.id, state);
        }

        self.cell_states = new_states;
    }

    /// Broadcast current graph state.
    fn broadcast_graph_update(&self) {
        let edges: Vec<DependencyEdge> = self
            .cells
            .iter()
            .flat_map(|cell| {
                cell.dependencies.iter().filter_map(|dep| {
                    self.cells
                        .iter()
                        .find(|c| c.name == dep.param_name)
                        .map(|producer| DependencyEdge {
                            from: producer.id,
                            to: cell.id,
                            param_name: dep.param_name.clone(),
                        })
                })
            })
            .collect();

        let order = match self.graph.topological_order() {
            Ok(order) => order,
            Err(e) => {
                tracing::error!("Failed to compute topological order: {}", e);
                Vec::new()
            }
        };
        let levels = self.graph.topological_levels(&order);

        self.broadcast(ServerMessage::GraphUpdated { edges, levels });
    }

    /// Get the full notebook state.
    pub fn get_state(&self) -> ServerMessage {
        let order = match self.graph.topological_order() {
            Ok(order) => order,
            Err(e) => {
                tracing::error!("Failed to compute execution order: {}", e);
                Vec::new()
            }
        };

        ServerMessage::NotebookState {
            path: self.path.display().to_string(),
            cells: self.cell_states.values().cloned().collect(),
            execution_order: order,
        }
    }

    /// Execute a specific cell.
    ///
    /// Uses process isolation - the cell runs in a worker process that can
    /// be killed immediately for interruption.
    pub async fn execute_cell(&mut self, cell_id: CellId) -> ServerResult<()> {
        if self.executing {
            return Err(ServerError::ExecutionInProgress);
        }

        let cell = self
            .get_cell(cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?
            .clone();

        self.executing = true;

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
                self.executor.register_cell(compiled, cell.dependencies.len());

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
                let exec_result = self.executor.execute_cell_with_widgets(
                    cell_id,
                    &inputs,
                    widget_values_json,
                );

                let duration = start.elapsed();

                match exec_result {
                    Ok((output, widgets_json)) => {
                        // Store output for dependent cells
                        let output_arc = Arc::new(output);
                        self.cell_outputs.insert(cell_id, output_arc.clone());

                        // Also store in executor state for consistency
                        self.executor.state_mut().store_output(cell_id, (*output_arc).clone());

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
                            state.status = CellStatus::Success;
                            state.output = Some(cell_output.clone());
                            state.dirty = false;
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
                            self.broadcast(ServerMessage::ExecutionAborted { cell_id: Some(cell_id) });
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
            if let Some(max_duration) = timeout {
                if start.elapsed() > max_duration {
                    self.executor.abort();
                    self.broadcast(ServerMessage::ExecutionAborted { cell_id: Some(cell_id) });
                    return Err(ServerError::ExecutionTimeout);
                }
            }

            self.execute_cell(cell_id).await?;
        }
        Ok(())
    }

    /// Mark a cell as dirty (needs re-execution).
    pub fn mark_dirty(&mut self, cell_id: CellId) {
        if let Some(state) = self.cell_states.get_mut(&cell_id) {
            state.dirty = true;
        }

        // Also mark dependents as dirty
        let dependents = self.graph.invalidated_cells(cell_id);
        for dep_id in dependents {
            if let Some(state) = self.cell_states.get_mut(&dep_id) {
                state.dirty = true;
            }
        }
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

    /// Get IDs of all dirty cells in topological order.
    pub fn get_dirty_cell_ids(&self) -> Vec<CellId> {
        let order = match self.graph.topological_order() {
            Ok(order) => order,
            Err(e) => {
                tracing::error!("Failed to compute order for dirty cells: {}", e);
                Vec::new()
            }
        };
        order
            .into_iter()
            .filter(|id| self.cell_states.get(id).is_some_and(|state| state.dirty))
            .collect()
    }

    /// Update a widget value for a cell.
    ///
    /// This stores the new value but does NOT trigger re-execution.
    /// The user must explicitly run the cell to see the effect.
    pub fn update_widget_value(&mut self, cell_id: CellId, widget_id: String, value: WidgetValue) {
        self.widget_values
            .entry(cell_id)
            .or_default()
            .insert(widget_id, value);
    }

    /// Get widget values for a cell.
    pub fn get_widget_values(&self, cell_id: CellId) -> HashMap<String, WidgetValue> {
        self.widget_values
            .get(&cell_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Get ALL widget values from all cells, flattened into a single map.
    /// Widget IDs should be unique across the notebook.
    pub fn get_all_widget_values(&self) -> HashMap<String, WidgetValue> {
        let mut all_values = HashMap::new();
        for cell_widgets in self.widget_values.values() {
            for (widget_id, value) in cell_widgets {
                all_values.insert(widget_id.clone(), value.clone());
            }
        }
        all_values
    }

    /// Get widget definitions for a cell.
    pub fn get_widget_defs(&self, cell_id: CellId) -> Vec<WidgetDef> {
        self.widget_defs
            .get(&cell_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Store widget definitions from cell execution.
    fn store_widget_defs(&mut self, cell_id: CellId, widgets: Vec<WidgetDef>) {
        if widgets.is_empty() {
            self.widget_defs.remove(&cell_id);
        } else {
            self.widget_defs.insert(cell_id, widgets);
        }
    }

    /// Add an execution result to history.
    fn add_to_history(&mut self, cell_id: CellId, serialized: Arc<BoxedOutput>, display: CellOutput) {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let entry = OutputHistoryEntry {
            serialized,
            display,
            timestamp,
        };

        let history = self.cell_output_history.entry(cell_id).or_insert_with(Vec::new);
        history.push(entry);

        // Trim if too long
        while history.len() > MAX_HISTORY_PER_CELL {
            history.remove(0);
        }

        // Set current index to the latest entry
        self.cell_history_index.insert(cell_id, history.len() - 1);
    }

    /// Select a history entry for a cell, making it the current output.
    /// Returns the display output if successful.
    pub fn select_history_entry(&mut self, cell_id: CellId, index: usize) -> Option<CellOutput> {
        // Clone what we need before doing any mutations (to avoid borrow conflicts)
        let (serialized, display) = {
            let history = self.cell_output_history.get(&cell_id)?;
            let entry = history.get(index)?;
            (entry.serialized.clone(), entry.display.clone())
        };

        // Update the current output for dependent cells
        self.cell_outputs.insert(cell_id, serialized.clone());
        self.executor.state_mut().store_output(cell_id, (*serialized).clone());

        // Update the cell state
        if let Some(state) = self.cell_states.get_mut(&cell_id) {
            state.output = Some(display.clone());
        }

        // Update history index
        self.cell_history_index.insert(cell_id, index);

        // Mark dependent cells as dirty
        self.mark_dependents_dirty(cell_id);

        Some(display)
    }

    /// Mark all cells that depend on the given cell as dirty.
    fn mark_dependents_dirty(&mut self, cell_id: CellId) {
        // Use the graph's invalidated_cells which returns all dependents
        let dependents = self.graph.invalidated_cells(cell_id);

        // Skip the first one (the changed cell itself) and mark the rest as dirty
        for dep_id in dependents.into_iter().skip(1) {
            if let Some(state) = self.cell_states.get_mut(&dep_id) {
                state.dirty = true;
            }
        }
    }

    /// Get history count for a cell.
    pub fn get_history_count(&self, cell_id: CellId) -> usize {
        self.cell_output_history.get(&cell_id).map(|h| h.len()).unwrap_or(0)
    }

    /// Get current history index for a cell.
    pub fn get_history_index(&self, cell_id: CellId) -> usize {
        self.cell_history_index.get(&cell_id).copied().unwrap_or(0)
    }

    /// Get reference to cell states.
    pub fn cell_states(&self) -> &HashMap<CellId, CellState> {
        &self.cell_states
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        // This would require a real notebook file, so we just test the types compile
        let (tx, _rx) = broadcast::channel::<ServerMessage>(16);
        drop(tx);
    }
}
