//! Notebook session management.
//!
//! Manages the state of an active notebook session including
//! compilation, execution, and output caching.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
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
use venus_core::graph::{CellId, CellInfo, CellParser, CellType, DefinitionCell, GraphEngine, MarkdownCell, MoveDirection, SourceEditor};
use venus_core::paths::NotebookDirs;

use crate::error::{ServerError, ServerResult};
use crate::protocol::{CellOutput, CellState, CellStatus, ServerMessage};
use crate::undo::{UndoManager, UndoableOperation};
use venus_core::state::BoxedOutput;

/// Find workspace root by walking up from notebook path to find Cargo.toml.
/// Returns (workspace_root, cargo_toml_path).
fn find_workspace_root(notebook_path: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let mut current = notebook_path.parent();

    while let Some(dir) = current {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            return (Some(dir.to_path_buf()), Some(cargo_toml));
        }
        current = dir.parent();
    }

    (None, None)
}

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

    /// Path to workspace Cargo.toml (if found).
    workspace_cargo_toml: Option<PathBuf>,

    /// Parsed code cells.
    cells: Vec<CellInfo>,

    /// Parsed markdown cells.
    markdown_cells: Vec<MarkdownCell>,

    /// Parsed definition cells (imports, types, helpers).
    definition_cells: Vec<DefinitionCell>,

    /// Dependency graph.
    graph: GraphEngine,

    /// Cell states for clients (both code and markdown).
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

    /// Undo/redo manager for cell operations.
    undo_manager: UndoManager,

    /// Pending edits from the editor (not yet saved to disk).
    /// These are saved to disk when the cell is executed.
    pending_edits: HashMap<CellId, String>,
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

        // Find workspace Cargo.toml (if it exists)
        let (_workspace_root, workspace_cargo_toml) = find_workspace_root(&path);

        // Set up directories using shared abstraction
        let dirs = NotebookDirs::from_notebook_path(&path)?;

        let toolchain = ToolchainManager::new()?;
        let config = CompilerConfig::for_notebook(&dirs);

        let (tx, rx) = broadcast::channel(MESSAGE_CHANNEL_CAPACITY);

        // Create process executor with warm worker pool
        let executor = ProcessExecutor::new(&dirs.state_dir)?;

        let mut session = Self {
            path,
            workspace_cargo_toml,
            cells: Vec::new(),
            markdown_cells: Vec::new(),
            definition_cells: Vec::new(),
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
            undo_manager: UndoManager::new(),
            pending_edits: HashMap::new(),
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

    /// Set the status of a code cell.
    fn set_cell_status(&mut self, cell_id: CellId, status: CellStatus) {
        if let Some(CellState::Code { status: cell_status, .. }) = self.cell_states.get_mut(&cell_id) {
            *cell_status = status;
        }
    }

    /// Broadcast a server message, ignoring send failures.
    pub fn broadcast(&self, msg: ServerMessage) {
        let _ = self.tx.send(msg);
    }

    /// Reload the notebook from disk.
    pub fn reload(&mut self) -> ServerResult<()> {
        let source = std::fs::read_to_string(&self.path)?;

        // Parse cells (code, markdown, and definitions)
        let mut parser = CellParser::new();
        let parse_result = parser.parse_file(&self.path)?;
        self.cells = parse_result.code_cells;
        self.markdown_cells = parse_result.markdown_cells;
        self.definition_cells = parse_result.definition_cells;

        // Build graph and update code cells with real IDs (parser returns placeholder IDs)
        self.graph = GraphEngine::new();
        for cell in &mut self.cells {
            let real_id = self.graph.add_cell(cell.clone());
            cell.id = real_id;
        }
        self.graph.resolve_dependencies()?;

        // Assign unique IDs to markdown cells (they don't participate in the dependency graph)
        let mut next_id = if let Some(max_code_id) = self.cells.iter().map(|c| c.id.as_usize()).max() {
            max_code_id + 1
        } else {
            0
        };
        for md_cell in &mut self.markdown_cells {
            md_cell.id = CellId::new(next_id);
            next_id += 1;
        }

        // Assign unique IDs to definition cells
        for def_cell in &mut self.definition_cells {
            def_cell.id = CellId::new(next_id);
            next_id += 1;
        }

        // Write virtual notebook.rs file for LSP analysis BEFORE building universe
        // This ensures the file exists when universe is compiled (lib.rs includes `pub mod notebook;`)
        if let Err(e) = self.write_virtual_notebook_file() {
            tracing::warn!("Failed to write virtual notebook file: {}", e);
        }

        // Build universe (always needed for bincode/serde runtime)
        let mut universe_builder =
            UniverseBuilder::new(self.config.clone(), self.toolchain.clone(), self.workspace_cargo_toml.clone());
        universe_builder.parse_dependencies(&source, &self.definition_cells)?;

        self.universe_path = Some(universe_builder.build()?);
        self.deps_hash = universe_builder.deps_hash();

        // Update cell states
        self.update_cell_states();

        // NOTE: We do NOT broadcast state here because reload() is called by the file watcher
        // when the notebook file changes (e.g., editor auto-save). Broadcasting on every file
        // change causes the UI to refresh continuously. Instead, each cell operation (insert,
        // edit, delete, etc.) explicitly broadcasts state after calling reload().

        Ok(())
    }

    /// Strip the first heading from a doc comment (since it's used as display name).
    ///
    /// If the doc comment starts with `# Heading`, removes that line and returns
    /// the rest of the content. Otherwise returns the original content.
    fn strip_display_name_from_description(doc_comment: &Option<String>) -> Option<String> {
        doc_comment.as_ref().and_then(|doc| {
            let lines: Vec<&str> = doc.lines().collect();

            // Find the first heading line
            if let Some(first_line) = lines.first() {
                let trimmed = first_line.trim();
                if trimmed.starts_with('#') {
                    // Skip the heading line and return the rest
                    let remaining: Vec<&str> = lines.iter().skip(1).copied()
                        .collect();

                    // Trim leading empty lines
                    let trimmed_lines: Vec<&str> = remaining.iter()
                        .skip_while(|line| line.trim().is_empty()).copied()
                        .collect();

                    if trimmed_lines.is_empty() {
                        return None;
                    }

                    return Some(trimmed_lines.join("\n"));
                }
            }

            // No heading found, return original
            Some(doc.clone())
        })
    }

    /// Update cell states from parsed cells.
    fn update_cell_states(&mut self) {
        let mut new_states = HashMap::new();

        // Add code cells
        for cell in &self.cells {
            let existing = self.cell_states.get(&cell.id);

            // Extract status, output, dirty from existing state if it's a code cell
            let (status, output, dirty) = if let Some(CellState::Code { status, output, dirty, .. }) = existing {
                (*status, output.clone(), *dirty)
            } else {
                // New cells start pristine: no output, not dirty
                (CellStatus::default(), None, false)
            };

            let state = CellState::Code {
                id: cell.id,
                name: cell.name.clone(),
                display_name: cell.display_name.clone(),
                source: cell.source_code.clone(),
                description: Self::strip_display_name_from_description(&cell.doc_comment),
                return_type: cell.return_type.clone(),
                dependencies: cell
                    .dependencies
                    .iter()
                    .map(|d| d.param_name.clone())
                    .collect(),
                status,
                output,
                dirty,
            };
            new_states.insert(cell.id, state);
        }

        // Add markdown cells
        for md_cell in &self.markdown_cells {
            let state = CellState::Markdown {
                id: md_cell.id,
                content: md_cell.content.clone(),
            };
            new_states.insert(md_cell.id, state);
        }

        // Add definition cells
        for def_cell in &self.definition_cells {
            let state = CellState::Definition {
                id: def_cell.id,
                content: def_cell.content.clone(),
                definition_type: def_cell.definition_type,
                doc_comment: def_cell.doc_comment.clone(),
            };
            new_states.insert(def_cell.id, state);
        }

        self.cell_states = new_states;
    }

    /// Write virtual notebook.rs file for LSP analysis.
    /// This file contains all cell content in source order so rust-analyzer can analyze it.
    /// Collect all cells (code and definition) in source order.
    /// Returns a vector of (cell_id, start_line, cell_type) tuples sorted by line number.
    /// Includes all cell types: code, markdown, and definition cells.
    fn collect_cells_in_source_order(&self) -> Vec<(CellId, usize, CellType)> {
        let mut all_cells: Vec<(CellId, usize, CellType)> = Vec::new();

        for cell in &self.cells {
            all_cells.push((cell.id, cell.span.start_line, CellType::Code));
        }

        for md_cell in &self.markdown_cells {
            all_cells.push((md_cell.id, md_cell.span.start_line, CellType::Markdown));
        }

        for def_cell in &self.definition_cells {
            all_cells.push((def_cell.id, def_cell.span.start_line, CellType::Definition));
        }

        all_cells.sort_by_key(|(_, line, _)| *line);
        all_cells
    }

    fn write_virtual_notebook_file(&self) -> std::io::Result<()> {
        use std::fs;

        let dirs = NotebookDirs::from_notebook_path(&self.path)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let universe_src = dirs.build_dir.join("universe").join("src");
        fs::create_dir_all(&universe_src)?;

        let mut lines = Vec::new();
        let all_cells = self.collect_cells_in_source_order();

        // Build combined source
        for (cell_id, _, cell_type) in all_cells {
            match cell_type {
                CellType::Code => {
                    if let Some(cell) = self.cells.iter().find(|c| c.id == cell_id) {
                        lines.push(cell.source_code.clone());
                        lines.push(String::new()); // Empty line between cells
                    }
                }
                CellType::Definition => {
                    if let Some(def_cell) = self.definition_cells.iter().find(|c| c.id == cell_id) {
                        lines.push(def_cell.content.clone());
                        lines.push(String::new()); // Empty line between cells
                    }
                }
                _ => {} // Ignore markdown cells
            }
        }

        let content = lines.join("\n");
        fs::write(universe_src.join("notebook.rs"), content)?;

        Ok(())
    }

    /// Get the full notebook state.
    /// Returns a snapshot of the current notebook state for UI rendering.
    /// Note: The virtual notebook.rs file for LSP is written during reload(), not here.
    pub fn get_state(&self) -> ServerMessage {
        // Source order: all cells (code + markdown + definition) in the order they appear in the .rs file
        let all_cells = self.collect_cells_in_source_order();
        let source_order: Vec<CellId> = all_cells.into_iter().map(|(id, _, _)| id).collect();

        // Execution order: topologically sorted for dependency resolution (code cells only)
        let execution_order = match self.graph.topological_order() {
            Ok(order) => order,
            Err(e) => {
                tracing::error!("Failed to compute execution order: {}", e);
                Vec::new()
            }
        };

        // Find workspace root by walking up from notebook path to find Cargo.toml
        let (workspace_root, cargo_toml_path) = find_workspace_root(&self.path);

        ServerMessage::NotebookState {
            path: self.path.display().to_string(),
            cells: self.cell_states.values().cloned().collect(),
            source_order,
            execution_order,
            workspace_root: workspace_root.map(|p| p.display().to_string()),
            cargo_toml_path: cargo_toml_path.map(|p| p.display().to_string()),
        }
    }

    /// Store a pending edit from the editor (not yet saved to disk).
    ///
    /// The edit will be saved to disk when the cell is executed.
    pub fn store_pending_edit(&mut self, cell_id: CellId, source: String) {
        self.pending_edits.insert(cell_id, source);
    }

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
                        // Check if output changed (for smart dirty marking)
                        let old_hash = self.cell_outputs.get(&cell_id)
                            .map(|old| Self::output_hash(old));
                        let new_hash = Self::output_hash(&output);
                        let output_changed = old_hash.is_none_or(|h| h != new_hash);

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
            if timeout.is_some_and(|max_duration| start.elapsed() > max_duration) {
                self.executor.abort();
                self.broadcast(ServerMessage::ExecutionAborted { cell_id: Some(cell_id) });
                return Err(ServerError::ExecutionTimeout);
            }

            self.execute_cell(cell_id).await?;
        }
        Ok(())
    }

    /// Mark a cell as dirty (needs re-execution).
    ///
    /// Only marks cells as dirty if they have existing output (data).
    /// Cells without output remain pristine (no border).
    pub fn mark_dirty(&mut self, cell_id: CellId) {
        // Mark the edited cell as dirty only if it has output
        if self.cell_outputs.contains_key(&cell_id) {
            if let Some(state) = self.cell_states.get_mut(&cell_id) {
                state.set_dirty(true);
            }
        }

        // Also mark dependents as dirty (only those with output)
        let dependents = self.graph.invalidated_cells(cell_id);
        for dep_id in dependents {
            if self.cell_outputs.contains_key(&dep_id) {
                if let Some(state) = self.cell_states.get_mut(&dep_id) {
                    state.set_dirty(true);
                }
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

    /// Clear all cell outputs without restarting the kernel.
    ///
    /// This clears the display outputs but preserves:
    /// - Worker pool and execution state
    /// - Widget values
    /// - Cell source code
    ///
    /// All cells are reset to pristine state (no output, not dirty).
    pub fn clear_outputs(&mut self) {
        // Clear outputs from cell states - back to pristine (not dirty)
        for state in self.cell_states.values_mut() {
            state.clear_output();
            state.set_dirty(false); // Pristine - no data, no dirty
            state.set_status(CellStatus::Idle);
        }

        // Clear cached outputs
        self.cell_outputs.clear();
        let _ = self.executor.state_mut().clear();

        // Clear output history
        self.cell_output_history.clear();
        self.cell_history_index.clear();

        // Broadcast outputs cleared message
        self.broadcast(ServerMessage::OutputsCleared { error: None });

        // Send updated state to all clients
        let state_msg = self.get_state();
        self.broadcast(state_msg);
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
            .filter(|id| self.cell_states.get(id).is_some_and(|state| state.is_dirty()))
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

        let history = self.cell_output_history.entry(cell_id).or_default();
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
            state.set_output(Some(display.clone()));
        }

        // Update history index
        self.cell_history_index.insert(cell_id, index);

        // Mark dependent cells as dirty
        let _ = self.mark_dependents_dirty_and_get(cell_id);

        Some(display)
    }

    /// Mark all cells that depend on the given cell as dirty.
    ///
    /// Only marks cells that have existing output (data). Cells without
    /// output remain pristine (no border) since they haven't been executed yet.
    /// Returns the list of cells that were marked dirty.
    fn mark_dependents_dirty_and_get(&mut self, cell_id: CellId) -> Vec<CellId> {
        // Use the graph's invalidated_cells which returns all dependents
        let dependents = self.graph.invalidated_cells(cell_id);
        let mut dirty_cells = Vec::new();

        // Skip the first one (the changed cell itself) and mark the rest as dirty
        // BUT only if they have output (data) - pristine cells stay pristine
        for dep_id in dependents.into_iter().skip(1) {
            // Only mark dirty if cell has output (has been executed before)
            if self.cell_outputs.contains_key(&dep_id) {
                if let Some(state) = self.cell_states.get_mut(&dep_id) {
                    state.set_dirty(true);
                    dirty_cells.push(dep_id);
                }
            }
        }
        dirty_cells
    }

    /// Compute a hash of output bytes for change detection.
    fn output_hash(output: &BoxedOutput) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        output.bytes().hash(&mut hasher);
        hasher.finish()
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

    /// Insert a new cell after the specified cell.
    ///
    /// Modifies the source file and triggers a reload.
    /// Returns the name of the newly created cell.
    pub fn insert_cell(&mut self, after_cell_id: Option<CellId>) -> ServerResult<String> {
        // Convert CellId to cell name if provided
        let after_name = after_cell_id.and_then(|id| {
            self.cells.iter().find(|c| c.id == id).map(|c| c.name.clone())
        });

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        let new_name = editor.insert_cell(after_name.as_deref())?;
        editor.save()?;

        // Record for undo (with position for redo)
        self.undo_manager.record(UndoableOperation::InsertCell {
            cell_name: new_name.clone(),
            after_cell_name: after_name,
        });

        // File watcher will trigger reload, but we can also reload now
        // to ensure immediate consistency
        self.reload()?;

        Ok(new_name)
    }

    /// Delete a cell from the notebook.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn delete_cell(&mut self, cell_id: CellId) -> ServerResult<()> {
        // Find the cell name
        let cell_name = self.cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| c.name.clone())
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        // Check if any other cells depend on this cell
        let dependents: Vec<String> = self.cells
            .iter()
            .filter(|c| c.id != cell_id) // Don't check self
            .filter(|c| {
                c.dependencies
                    .iter()
                    .any(|dep| dep.param_name == cell_name)
            })
            .map(|c| c.name.clone())
            .collect();

        if !dependents.is_empty() {
            return Err(ServerError::InvalidOperation(format!(
                "Cannot delete cell '{}' because it is used by: {}",
                cell_name,
                dependents.join(", ")
            )));
        }

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;

        // Capture source and position before deletion (for undo)
        let source = editor.get_cell_source(&cell_name)?;
        let after_cell_name = editor.get_previous_cell_name(&cell_name)?;

        editor.delete_cell(&cell_name)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::DeleteCell {
            cell_name: cell_name.clone(),
            source,
            after_cell_name,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Duplicate a cell in the notebook.
    ///
    /// Creates a copy of the cell with a unique name.
    /// Returns the name of the new cell.
    pub fn duplicate_cell(&mut self, cell_id: CellId) -> ServerResult<String> {
        // Find the cell name
        let cell_name = self.cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| c.name.clone())
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        let new_name = editor.duplicate_cell(&cell_name)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::DuplicateCell {
            original_cell_name: cell_name,
            new_cell_name: new_name.clone(),
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(new_name)
    }

    /// Move a cell up or down in the notebook.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn move_cell(&mut self, cell_id: CellId, direction: MoveDirection) -> ServerResult<()> {
        // Find the cell name
        let cell_name = self.cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| c.name.clone())
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.move_cell(&cell_name, direction)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::MoveCell {
            cell_name,
            direction,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Edit a code cell's source.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn edit_cell(&mut self, cell_id: CellId, new_source: String) -> ServerResult<()> {
        // Find the cell
        let cell = self.cells
            .iter()
            .find(|c| c.id == cell_id)
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        let cell_name = cell.name.clone();
        let old_source = cell.source_code.clone();

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;

        // Reconstruct complete cell (doc comments + #[venus::cell] + function) and get FRESH line numbers
        let (reconstructed, start_line, end_line) = editor.reconstruct_and_get_span(&cell_name, &new_source)?;

        tracing::info!("Editing cell '{}' lines {}-{}, reconstructed length: {}", cell_name, start_line, end_line, reconstructed.len());
        editor.edit_raw_code(start_line, end_line, &reconstructed)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::EditCell {
            cell_id,
            start_line,
            end_line,
            old_source,
            new_source: new_source.clone(),
        });

        // Reload to update in-memory state
        // Save outputs by name BEFORE reload (IDs will change)
        let outputs_by_name: HashMap<String, Arc<BoxedOutput>> = self.cells.iter()
            .filter_map(|c| self.cell_outputs.get(&c.id).map(|o| (c.name.clone(), o.clone())))
            .collect();

        self.reload()?;

        // Restore outputs with NEW IDs (except for the edited cell)
        self.cell_outputs.clear();
        for cell in &self.cells {
            if cell.name != cell_name {
                if let Some(output) = outputs_by_name.get(&cell.name) {
                    self.cell_outputs.insert(cell.id, output.clone());
                }
            }
        }

        Ok(())
    }

    /// Rename a cell's display name.
    ///
    /// Updates the cell's doc comment with the new display name and reloads the notebook.
    pub fn rename_cell(&mut self, cell_id: CellId, new_display_name: String) -> ServerResult<()> {
        // Find the cell name and current display name
        let (cell_name, old_display_name) = self.cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| (c.name.clone(), c.display_name.clone()))
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.rename_cell(&cell_name, &new_display_name)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::RenameCell {
            cell_name,
            old_display_name,
            new_display_name,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Insert a new markdown cell.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn insert_markdown_cell(&mut self, content: String, after_cell_id: Option<CellId>) -> ServerResult<()> {
        // Convert cell ID to line number if provided
        let after_line = after_cell_id.and_then(|id| {
            // Try to find in code cells
            self.cells.iter().find(|c| c.id == id)
                .map(|c| c.span.end_line)
                .or_else(|| {
                    // Try to find in markdown cells
                    self.markdown_cells.iter().find(|m| m.id == id)
                        .map(|m| m.span.end_line)
                })
        });

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.insert_markdown_cell(&content, after_line)?;

        // Get the line range of the newly inserted cell (approximate)
        let start_line = after_line.map(|l| l + 1).unwrap_or(0);
        let line_count = content.lines().count();
        let end_line = start_line + line_count;

        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::InsertMarkdownCell {
            start_line,
            end_line,
            content: content.clone(),
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Edit a markdown cell's content.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn edit_markdown_cell(&mut self, cell_id: CellId, new_content: String) -> ServerResult<()> {
        // Find the markdown cell
        let md_cell = self.markdown_cells
            .iter()
            .find(|m| m.id == cell_id)
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        let start_line = md_cell.span.start_line;
        let end_line = md_cell.span.end_line;
        let old_content = md_cell.content.clone();
        let is_module_doc = md_cell.is_module_doc;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.edit_markdown_cell(start_line, end_line, &new_content, is_module_doc)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::EditMarkdownCell {
            start_line,
            end_line,
            old_content,
            new_content,
            is_module_doc,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Delete a markdown cell.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn delete_markdown_cell(&mut self, cell_id: CellId) -> ServerResult<()> {
        // Find the markdown cell
        let md_cell = self.markdown_cells
            .iter()
            .find(|m| m.id == cell_id)
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        let start_line = md_cell.span.start_line;
        let end_line = md_cell.span.end_line;
        let content = md_cell.content.clone();

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.delete_markdown_cell(start_line, end_line)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::DeleteMarkdownCell {
            start_line,
            content,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Move a markdown cell up or down.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn move_markdown_cell(&mut self, cell_id: CellId, direction: MoveDirection) -> ServerResult<()> {
        // Find the markdown cell
        let md_cell = self.markdown_cells
            .iter()
            .find(|m| m.id == cell_id)
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        let start_line = md_cell.span.start_line;
        let end_line = md_cell.span.end_line;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.move_markdown_cell(start_line, end_line, direction)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::MoveMarkdownCell {
            start_line,
            end_line,
            direction,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Infer the definition type from content for validation.
    ///
    /// Provides early error detection when users specify an incorrect definition type.
    /// This is a best-effort heuristic based on content analysis.
    ///
    /// Returns `None` if the type cannot be reliably inferred.
    fn infer_definition_type(content: &str) -> Option<venus_core::graph::DefinitionType> {
        use venus_core::graph::DefinitionType;

        let trimmed = content.trim();

        // Check for import statements (use declarations)
        if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
            return Some(DefinitionType::Import);
        }

        // Check for struct definitions
        if trimmed.contains("struct ") {
            return Some(DefinitionType::Struct);
        }

        // Check for enum definitions
        if trimmed.contains("enum ") {
            return Some(DefinitionType::Enum);
        }

        // Check for type alias (but not inside a function)
        if trimmed.contains("type ") && !trimmed.contains("fn ") {
            return Some(DefinitionType::TypeAlias);
        }

        // Check for function definitions (without #[venus::cell])
        if trimmed.contains("fn ") && !trimmed.contains("#[venus::cell]") {
            return Some(DefinitionType::HelperFunction);
        }

        None
    }

    /// Validate that the declared definition type matches the content.
    ///
    /// Provides a warning if there's a mismatch, but doesn't fail the operation
    /// since the actual parsing will catch any real errors during universe build.
    ///
    /// Returns `Ok(())` if valid or cannot be validated, `Err` only for clear mismatches.
    fn validate_definition_type(
        content: &str,
        declared_type: venus_core::graph::DefinitionType,
    ) -> ServerResult<()> {
        if let Some(inferred_type) = Self::infer_definition_type(content)
            && std::mem::discriminant(&inferred_type) != std::mem::discriminant(&declared_type) {
                tracing::warn!(
                    "Definition type mismatch: declared {:?} but content suggests {:?}",
                    declared_type,
                    inferred_type
                );
                // For now, just warn - don't fail the operation
                // The universe build will catch actual syntax errors
            }
        Ok(())
    }

    /// Insert a new definition cell.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    /// Returns the ID of the newly inserted definition cell.
    ///
    /// Validates that the definition type matches the content before insertion.
    pub fn insert_definition_cell(
        &mut self,
        content: String,
        definition_type: venus_core::graph::DefinitionType,
        after_cell_id: Option<CellId>,
    ) -> ServerResult<CellId> {
        // Validate definition type matches content
        Self::validate_definition_type(&content, definition_type)?;
        // Convert cell ID to line number if provided
        let after_line = after_cell_id.and_then(|id| {
            // Try to find in code cells
            self.cells.iter().find(|c| c.id == id)
                .map(|c| c.span.end_line)
                .or_else(|| {
                    // Try to find in markdown cells
                    self.markdown_cells.iter().find(|m| m.id == id)
                        .map(|m| m.span.end_line)
                })
                .or_else(|| {
                    // Try to find in definition cells
                    self.definition_cells.iter().find(|d| d.id == id)
                        .map(|d| d.span.end_line)
                })
        });

        // Use insert_raw_code which writes raw Rust code without // prefix
        let mut editor = SourceEditor::load(&self.path)?;
        editor.insert_raw_code(&content, after_line)?;

        let start_line = after_line.map(|l| l + 1).unwrap_or(0);
        let line_count = content.lines().count();
        let end_line = start_line + line_count;

        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::InsertDefinitionCell {
            start_line,
            end_line,
            content: content.clone(),
            definition_type,
        });

        // Reload to update in-memory state
        self.reload()?;

        // Find the newly inserted definition cell (it should be at the expected line)
        let new_cell_id = self.definition_cells
            .iter()
            .find(|d| d.span.start_line >= start_line && d.span.start_line <= end_line)
            .map(|d| d.id)
            .ok_or_else(|| ServerError::InvalidOperation("Failed to find inserted definition cell".to_string()))?;

        Ok(new_cell_id)
    }

    /// Edit a definition cell's content.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    /// Returns a list of cells that are now dirty due to the definition change.
    pub fn edit_definition_cell(&mut self, cell_id: CellId, new_content: String) -> ServerResult<Vec<CellId>> {
        // Find the definition cell
        let def_cell = self.definition_cells
            .iter()
            .find(|d| d.id == cell_id)
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        let start_line = def_cell.span.start_line;
        let end_line = def_cell.span.end_line;
        let old_content = def_cell.content.clone();

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        // Use edit_raw_code which edits raw Rust code without // prefix
        editor.edit_raw_code(start_line, end_line, &new_content)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::EditDefinitionCell {
            cell_id,
            start_line,
            end_line,
            old_content,
            new_content: new_content.clone(),
        });

        // Reload to update in-memory state (rebuilds universe with new definitions)
        self.reload()?;

        // Mark ALL executable cells as dirty (only if they have output - pristine cells stay pristine)
        let dirty_cells: Vec<CellId> = self.cells.iter()
            .filter(|c| self.cell_outputs.contains_key(&c.id))  // Only cells with output
            .map(|c| c.id)
            .collect();
        for &cell_id in &dirty_cells {
            if let Some(state) = self.cell_states.get_mut(&cell_id) {
                state.set_dirty(true);
            }
        }

        Ok(dirty_cells)
    }

    /// Delete a definition cell.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn delete_definition_cell(&mut self, cell_id: CellId) -> ServerResult<()> {
        // Find the definition cell
        let def_cell = self.definition_cells
            .iter()
            .find(|d| d.id == cell_id)
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        let start_line = def_cell.span.start_line;
        let end_line = def_cell.span.end_line;
        let content = def_cell.content.clone();
        let definition_type = def_cell.definition_type;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.delete_markdown_cell(start_line, end_line)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::DeleteDefinitionCell {
            start_line,
            end_line,
            content,
            definition_type,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Move a definition cell up or down.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn move_definition_cell(&mut self, cell_id: CellId, direction: MoveDirection) -> ServerResult<()> {
        // Find the definition cell
        let def_cell = self.definition_cells
            .iter()
            .find(|d| d.id == cell_id)
            .ok_or_else(|| ServerError::CellNotFound(cell_id))?;

        let start_line = def_cell.span.start_line;
        let end_line = def_cell.span.end_line;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.move_markdown_cell(start_line, end_line, direction)?;
        editor.save()?;

        // Record for undo
        self.undo_manager.record(UndoableOperation::MoveDefinitionCell {
            start_line,
            end_line,
            direction,
        });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }

    /// Undo the last cell management operation.
    ///
    /// Returns a description of what was undone, or an error if undo failed.
    pub fn undo(&mut self) -> ServerResult<String> {
        let operation = self.undo_manager.pop_undo()
            .ok_or_else(|| ServerError::InvalidOperation("Nothing to undo".to_string()))?;

        let description = operation.undo_description();

        // Execute the reverse operation
        let mut editor = SourceEditor::load(&self.path)?;

        match &operation {
            UndoableOperation::InsertCell { cell_name, .. } => {
                // Undo insert = delete
                editor.delete_cell(cell_name)?;
            }
            UndoableOperation::DeleteCell { source, after_cell_name, .. } => {
                // Undo delete = restore
                editor.restore_cell(source, after_cell_name.as_deref())?;
            }
            UndoableOperation::DuplicateCell { new_cell_name, .. } => {
                // Undo duplicate = delete the new cell
                editor.delete_cell(new_cell_name)?;
            }
            UndoableOperation::MoveCell { cell_name, direction } => {
                // Undo move = move in opposite direction
                let reverse_direction = match direction {
                    MoveDirection::Up => MoveDirection::Down,
                    MoveDirection::Down => MoveDirection::Up,
                };
                editor.move_cell(cell_name, reverse_direction)?;
            }
            UndoableOperation::RenameCell { cell_name, old_display_name, .. } => {
                // Undo rename = restore old display name
                editor.rename_cell(cell_name, old_display_name)?;
            }
            UndoableOperation::EditCell { start_line, end_line, old_source, .. } => {
                // Undo edit = restore old source
                editor.edit_raw_code(*start_line, *end_line, old_source)?;
            }
            UndoableOperation::InsertMarkdownCell { start_line, end_line, .. } => {
                // Undo insert markdown = delete it
                editor.delete_markdown_cell(*start_line, *end_line)?;
            }
            UndoableOperation::EditMarkdownCell { start_line, end_line, old_content, is_module_doc, .. } => {
                // Undo edit markdown = restore old content
                editor.edit_markdown_cell(*start_line, *end_line, old_content, *is_module_doc)?;
            }
            UndoableOperation::DeleteMarkdownCell { start_line, content } => {
                // Undo delete markdown = restore it
                let after_line = if *start_line > 0 { Some(start_line - 1) } else { None };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::MoveMarkdownCell { start_line, end_line, direction } => {
                // Undo move markdown = move in opposite direction
                let reverse_direction = match direction {
                    MoveDirection::Up => MoveDirection::Down,
                    MoveDirection::Down => MoveDirection::Up,
                };
                editor.move_markdown_cell(*start_line, *end_line, reverse_direction)?;
            }
            UndoableOperation::InsertDefinitionCell { start_line, end_line, .. } => {
                // Undo insert definition = delete it
                editor.delete_markdown_cell(*start_line, *end_line)?;
            }
            UndoableOperation::EditDefinitionCell { start_line, end_line, old_content, .. } => {
                // Undo edit definition = restore old content
                editor.edit_markdown_cell(*start_line, *end_line, old_content, false)?;
            }
            UndoableOperation::DeleteDefinitionCell { start_line, content, .. } => {
                // Undo delete definition = restore it
                let after_line = if *start_line > 0 { Some(start_line - 1) } else { None };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::MoveDefinitionCell { start_line, end_line, direction } => {
                // Undo move definition = move in opposite direction
                let reverse_direction = match direction {
                    MoveDirection::Up => MoveDirection::Down,
                    MoveDirection::Down => MoveDirection::Up,
                };
                editor.move_markdown_cell(*start_line, *end_line, reverse_direction)?;
            }
        }

        editor.save()?;

        // Record for redo
        self.undo_manager.record_redo(operation);

        // Reload to update in-memory state
        self.reload()?;

        Ok(description)
    }

    /// Redo the last undone operation.
    ///
    /// Returns a description of what was redone, or an error if redo failed.
    pub fn redo(&mut self) -> ServerResult<String> {
        let operation = self.undo_manager.pop_redo()
            .ok_or_else(|| ServerError::InvalidOperation("Nothing to redo".to_string()))?;

        let description = operation.description();

        // Execute the original operation
        let mut editor = SourceEditor::load(&self.path)?;

        match &operation {
            UndoableOperation::InsertCell { after_cell_name, .. } => {
                // Re-insert at the original position
                let _ = editor.insert_cell(after_cell_name.as_deref())?;
            }
            UndoableOperation::DeleteCell { cell_name, .. } => {
                // Redo delete = delete again
                editor.delete_cell(cell_name)?;
            }
            UndoableOperation::DuplicateCell { original_cell_name, .. } => {
                // Redo duplicate = duplicate again (new name will be generated)
                let _ = editor.duplicate_cell(original_cell_name)?;
            }
            UndoableOperation::MoveCell { cell_name, direction } => {
                // Redo move = move in same direction
                editor.move_cell(cell_name, *direction)?;
            }
            UndoableOperation::RenameCell { cell_name, new_display_name, .. } => {
                // Redo rename = apply new display name again
                editor.rename_cell(cell_name, new_display_name)?;
            }
            UndoableOperation::EditCell { start_line, end_line, new_source, .. } => {
                // Redo edit = apply new source again
                editor.edit_raw_code(*start_line, *end_line, new_source)?;
            }
            UndoableOperation::InsertMarkdownCell { start_line, content, .. } => {
                // Redo insert markdown = insert again at original position
                let after_line = if *start_line > 0 { Some(start_line - 1) } else { None };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::EditMarkdownCell { start_line, end_line, new_content, is_module_doc, .. } => {
                // Redo edit markdown = apply new content again
                editor.edit_markdown_cell(*start_line, *end_line, new_content, *is_module_doc)?;
            }
            UndoableOperation::DeleteMarkdownCell { start_line, content } => {
                // Redo delete markdown = delete again
                // We need to find the end line by counting content lines
                let line_count = content.lines().count();
                let end_line = start_line + line_count;
                editor.delete_markdown_cell(*start_line, end_line)?;
            }
            UndoableOperation::MoveMarkdownCell { start_line, end_line, direction } => {
                // Redo move markdown = move in same direction
                editor.move_markdown_cell(*start_line, *end_line, *direction)?;
            }
            UndoableOperation::InsertDefinitionCell { start_line, content, .. } => {
                // Redo insert definition = insert again at original position
                let after_line = if *start_line > 0 { Some(start_line - 1) } else { None };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::EditDefinitionCell { start_line, end_line, new_content, .. } => {
                // Redo edit definition = apply new content again
                editor.edit_markdown_cell(*start_line, *end_line, new_content, false)?;
            }
            UndoableOperation::DeleteDefinitionCell { start_line, content, .. } => {
                // Redo delete definition = delete again
                let line_count = content.lines().count();
                let end_line = start_line + line_count;
                editor.delete_markdown_cell(*start_line, end_line)?;
            }
            UndoableOperation::MoveDefinitionCell { start_line, end_line, direction } => {
                // Redo move definition = move in same direction
                editor.move_markdown_cell(*start_line, *end_line, *direction)?;
            }
        }

        editor.save()?;

        // Record for undo (so we can undo the redo)
        self.undo_manager.record(operation);

        // Reload to update in-memory state
        self.reload()?;

        Ok(description)
    }

    /// Get the current undo/redo state.
    pub fn get_undo_redo_state(&self) -> ServerMessage {
        ServerMessage::UndoRedoState {
            can_undo: self.undo_manager.can_undo(),
            can_redo: self.undo_manager.can_redo(),
            undo_description: self.undo_manager.undo_description(),
            redo_description: self.undo_manager.redo_description(),
        }
    }

    /// Clear undo/redo history.
    ///
    /// Called when the file is externally modified.
    pub fn clear_undo_history(&mut self) {
        self.undo_manager.clear();
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
