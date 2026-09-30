use super::NotebookSession;
use super::model::{InterruptFlag, MESSAGE_CHANNEL_CAPACITY};
use crate::error::{ServerError, ServerResult};
use crate::protocol::ServerMessage;
use crate::undo::UndoManager;
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::broadcast;
use venus_core::compile::{CompilerConfig, ToolchainManager, UniverseBuilder};
use venus_core::execute::ProcessExecutor;
use venus_core::graph::{CellId, CellParser, CellType, GraphEngine};
use venus_core::paths::NotebookDirs;

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

    /// Reload the notebook from disk.
    pub fn reload(&mut self) -> ServerResult<()> {
        let source = std::fs::read_to_string(&self.path)?;

        // Parse cells (code, markdown, and definitions)
        let mut parser = CellParser::new();
        let parse_result = parser.parse_str(&source, &self.path)?;
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
        let mut next_id =
            if let Some(max_code_id) = self.cells.iter().map(|c| c.id.as_usize()).max() {
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
            UniverseBuilder::for_notebook(self.config.clone(), self.toolchain.clone(), &self.path)?;
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
}
