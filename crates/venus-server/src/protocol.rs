//! WebSocket protocol messages for Venus server.
//!
//! Defines the message types exchanged between client and server.

use serde::{Deserialize, Serialize};
use venus::widgets::{WidgetDef, WidgetValue};
use venus_core::graph::{CellId, DefinitionType};

// Re-export MoveDirection from venus_core for use in protocol messages
pub use venus_core::graph::MoveDirection;

/// Messages sent from client to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Request current notebook state.
    GetState,

    /// Edit a cell's source code.
    CellEdit {
        /// Cell identifier.
        cell_id: CellId,
        /// New source code.
        source: String,
    },

    /// Execute a specific cell.
    ExecuteCell {
        /// Cell to execute.
        cell_id: CellId,
    },

    /// Execute all cells.
    ExecuteAll,

    /// Execute cells that need re-execution.
    ExecuteDirty,

    /// Interrupt running execution.
    Interrupt,

    /// Sync notebook to .ipynb format.
    Sync,

    /// Request dependency graph.
    GetGraph,

    /// Update a widget value.
    WidgetUpdate {
        /// Cell containing the widget.
        cell_id: CellId,
        /// Widget identifier within the cell.
        widget_id: String,
        /// New widget value.
        value: WidgetValue,
    },

    /// Select a history entry to use as the current output.
    SelectHistory {
        /// Cell to select history for.
        cell_id: CellId,
        /// History index (0 = oldest).
        index: usize,
    },

    /// Insert a new cell.
    InsertCell {
        /// Cell ID to insert after. None = insert at end.
        after_cell_id: Option<CellId>,
    },

    /// Delete a cell.
    DeleteCell {
        /// Cell to delete.
        cell_id: CellId,
    },

    /// Duplicate a cell.
    DuplicateCell {
        /// Cell to duplicate.
        cell_id: CellId,
    },

    /// Move a cell up or down.
    MoveCell {
        /// Cell to move.
        cell_id: CellId,
        /// Direction to move.
        direction: MoveDirection,
    },

    /// Undo the last cell management operation.
    Undo,

    /// Redo the last undone operation.
    Redo,

    /// Restart the kernel (kill WorkerPool, clear memory state, preserve source).
    RestartKernel,

    /// Clear all cell outputs without restarting the kernel.
    ClearOutputs,

    /// Rename a cell's display name.
    RenameCell {
        /// Cell to rename.
        cell_id: CellId,
        /// New display name.
        new_display_name: String,
    },

    /// Insert a new markdown cell.
    InsertMarkdownCell {
        /// Markdown content.
        content: String,
        /// Cell ID to insert after. None = insert at beginning.
        after_cell_id: Option<CellId>,
    },

    /// Edit a markdown cell's content.
    EditMarkdownCell {
        /// Cell to edit.
        cell_id: CellId,
        /// New markdown content.
        new_content: String,
    },

    /// Delete a markdown cell.
    DeleteMarkdownCell {
        /// Cell to delete.
        cell_id: CellId,
    },

    /// Move a markdown cell up or down.
    MoveMarkdownCell {
        /// Cell to move.
        cell_id: CellId,
        /// Direction to move.
        direction: MoveDirection,
    },

    /// Insert a new definition cell.
    InsertDefinitionCell {
        /// Definition content (source code).
        content: String,
        /// Type of definition.
        definition_type: DefinitionType,
        /// Cell ID to insert after. None = insert at beginning.
        after_cell_id: Option<CellId>,
    },

    /// Edit a definition cell's content.
    EditDefinitionCell {
        /// Cell to edit.
        cell_id: CellId,
        /// New definition content.
        new_content: String,
    },

    /// Delete a definition cell.
    DeleteDefinitionCell {
        /// Cell to delete.
        cell_id: CellId,
    },

    /// Move a definition cell up or down.
    MoveDefinitionCell {
        /// Cell to move.
        cell_id: CellId,
        /// Direction to move.
        direction: MoveDirection,
    },
}

/// Messages sent from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Full notebook state (sent on connection or refresh).
    NotebookState {
        /// Path to the notebook file.
        path: String,
        /// All cells in the notebook.
        cells: Vec<CellState>,
        /// Source order (cell IDs in the order they appear in the .rs file).
        source_order: Vec<CellId>,
        /// Execution order (topologically sorted cell IDs for dependency resolution).
        execution_order: Vec<CellId>,
        /// Path to the workspace root (directory containing Cargo.toml).
        workspace_root: Option<String>,
        /// Path to the Cargo.toml file for LSP configuration.
        cargo_toml_path: Option<String>,
    },

    /// Cell execution started.
    CellStarted {
        /// Cell that started executing.
        cell_id: CellId,
    },

    /// Cell execution completed successfully.
    CellCompleted {
        /// Cell that completed.
        cell_id: CellId,
        /// Execution time in milliseconds.
        duration_ms: u64,
        /// Cell output (serialized).
        output: Option<CellOutput>,
    },

    /// Cell marked as dirty (needs re-execution because upstream changed).
    CellDirty {
        /// Cell that is now dirty.
        cell_id: CellId,
    },

    /// Cell execution failed.
    CellError {
        /// Cell that failed.
        cell_id: CellId,
        /// Error message.
        error: String,
        /// Source location if available.
        location: Option<SourceLocation>,
    },

    /// Compilation error (before execution).
    CompileError {
        /// Cell with compilation error.
        cell_id: CellId,
        /// Compiler errors.
        errors: Vec<CompileErrorInfo>,
    },

    /// Dependency graph updated.
    GraphUpdated {
        /// New dependency edges.
        edges: Vec<DependencyEdge>,
        /// Parallel execution levels.
        levels: Vec<Vec<CellId>>,
    },

    /// Notebook file changed externally.
    FileChanged {
        /// Cells that were modified.
        modified_cells: Vec<CellId>,
        /// Cells that were added.
        added_cells: Vec<CellState>,
        /// Cells that were removed.
        removed_cells: Vec<CellId>,
    },

    /// Sync completed.
    SyncCompleted {
        /// Path to generated .ipynb file.
        ipynb_path: String,
    },

    /// Execution was aborted by user request.
    ExecutionAborted {
        /// The cell that was interrupted (if known).
        cell_id: Option<CellId>,
    },

    /// Generic error message.
    Error {
        /// Error description.
        message: String,
    },

    /// Cell insertion result.
    CellInserted {
        /// ID of the newly created cell.
        cell_id: CellId,
        /// Error message if insertion failed.
        error: Option<String>,
    },

    /// Cell deletion result.
    CellDeleted {
        /// ID of the deleted cell.
        cell_id: CellId,
        /// Error message if deletion failed.
        error: Option<String>,
    },

    /// Cell duplication result.
    CellDuplicated {
        /// ID of the original cell.
        original_cell_id: CellId,
        /// ID of the new duplicated cell.
        new_cell_id: CellId,
        /// Error message if duplication failed.
        error: Option<String>,
    },

    /// Cell move result.
    CellMoved {
        /// ID of the moved cell.
        cell_id: CellId,
        /// Error message if move failed.
        error: Option<String>,
    },

    /// History entry selected for a cell.
    HistorySelected {
        /// Cell whose history was changed.
        cell_id: CellId,
        /// New history index.
        index: usize,
        /// Total history count.
        count: usize,
        /// The output at this history entry.
        output: Option<CellOutput>,
        /// Cells that are now dirty (need re-execution).
        dirty_cells: Vec<CellId>,
    },

    /// Undo operation result.
    UndoResult {
        /// Whether the undo succeeded.
        success: bool,
        /// Error message if undo failed.
        error: Option<String>,
        /// Description of what was undone (e.g., "Deleted cell 'foo'").
        description: Option<String>,
    },

    /// Redo operation result.
    RedoResult {
        /// Whether the redo succeeded.
        success: bool,
        /// Error message if redo failed.
        error: Option<String>,
        /// Description of what was redone.
        description: Option<String>,
    },

    /// Current undo/redo state (sent after each operation).
    UndoRedoState {
        /// Whether undo is available.
        can_undo: bool,
        /// Whether redo is available.
        can_redo: bool,
        /// Description of what will be undone (for UI tooltip).
        undo_description: Option<String>,
        /// Description of what will be redone (for UI tooltip).
        redo_description: Option<String>,
    },

    /// Kernel restart completed.
    KernelRestarted {
        /// Error message if restart failed.
        error: Option<String>,
    },

    /// All outputs cleared.
    OutputsCleared {
        /// Error message if clear failed.
        error: Option<String>,
    },

    /// Cell rename result.
    CellRenamed {
        /// ID of the renamed cell.
        cell_id: CellId,
        /// New display name.
        new_display_name: String,
        /// Error message if rename failed.
        error: Option<String>,
    },

    /// Markdown cell insertion result.
    MarkdownCellInserted {
        /// ID of the newly created markdown cell.
        cell_id: CellId,
        /// Error message if insertion failed.
        error: Option<String>,
    },

    /// Markdown cell edit result.
    MarkdownCellEdited {
        /// ID of the edited markdown cell.
        cell_id: CellId,
        /// Error message if edit failed.
        error: Option<String>,
    },

    /// Markdown cell deletion result.
    MarkdownCellDeleted {
        /// ID of the deleted markdown cell.
        cell_id: CellId,
        /// Error message if deletion failed.
        error: Option<String>,
    },

    /// Markdown cell move result.
    MarkdownCellMoved {
        /// ID of the moved markdown cell.
        cell_id: CellId,
        /// Error message if move failed.
        error: Option<String>,
    },

    /// Definition cell insertion result.
    DefinitionCellInserted {
        /// ID of the newly created definition cell.
        cell_id: CellId,
        /// Error message if insertion failed.
        error: Option<String>,
    },

    /// Definition cell edit result.
    DefinitionCellEdited {
        /// ID of the edited definition cell.
        cell_id: CellId,
        /// Error message if edit failed.
        error: Option<String>,
        /// Cells that are now dirty (need re-execution) due to definition change.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        dirty_cells: Vec<CellId>,
    },

    /// Definition cell deletion result.
    DefinitionCellDeleted {
        /// ID of the deleted definition cell.
        cell_id: CellId,
        /// Error message if deletion failed.
        error: Option<String>,
    },

    /// Definition cell move result.
    DefinitionCellMoved {
        /// ID of the moved definition cell.
        cell_id: CellId,
        /// Error message if move failed.
        error: Option<String>,
    },
}

/// State of a single cell (code, markdown, or definition).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cell_type", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum CellState {
    /// Code cell (executable function).
    Code {
        /// Unique cell identifier.
        id: CellId,
        /// Cell name (function name).
        name: String,
        /// Human-readable display name.
        display_name: String,
        /// Cell source code.
        source: String,
        /// Doc comment / description.
        description: Option<String>,
        /// Return type.
        return_type: String,
        /// Dependencies (parameter names).
        dependencies: Vec<String>,
        /// Current execution status.
        status: CellStatus,
        /// Last output if available.
        output: Option<CellOutput>,
        /// Whether the cell needs re-execution.
        dirty: bool,
    },
    /// Markdown cell (non-executable documentation).
    Markdown {
        /// Unique cell identifier.
        id: CellId,
        /// Markdown content.
        content: String,
    },
    /// Definition cell (types, imports, helper functions - compiled into universe).
    Definition {
        /// Unique cell identifier.
        id: CellId,
        /// Definition content (source code).
        content: String,
        /// Type of definition.
        definition_type: DefinitionType,
        /// Attached doc comment.
        doc_comment: Option<String>,
    },
}

impl CellState {
    /// Get the cell ID.
    pub fn id(&self) -> CellId {
        match self {
            CellState::Code { id, .. }
            | CellState::Markdown { id, .. }
            | CellState::Definition { id, .. } => *id,
        }
    }

    /// Get the cell name (only for code cells).
    pub fn name(&self) -> Option<&str> {
        match self {
            CellState::Code { name, .. } => Some(name),
            CellState::Markdown { .. } | CellState::Definition { .. } => None,
        }
    }

    /// Check if cell is dirty (only code cells can be dirty).
    pub fn is_dirty(&self) -> bool {
        match self {
            CellState::Code { dirty, .. } => *dirty,
            CellState::Markdown { .. } | CellState::Definition { .. } => false,
        }
    }

    /// Set dirty flag (only for code cells).
    pub fn set_dirty(&mut self, value: bool) {
        if let CellState::Code { dirty, .. } = self {
            *dirty = value;
        }
    }

    /// Get status (only for code cells).
    pub fn status(&self) -> Option<CellStatus> {
        match self {
            CellState::Code { status, .. } => Some(*status),
            CellState::Markdown { .. } | CellState::Definition { .. } => None,
        }
    }

    /// Set status (only for code cells).
    pub fn set_status(&mut self, new_status: CellStatus) {
        if let CellState::Code { status, .. } = self {
            *status = new_status;
        }
    }

    /// Set output (only for code cells).
    pub fn set_output(&mut self, new_output: Option<CellOutput>) {
        if let CellState::Code { output, .. } = self {
            *output = new_output;
        }
    }

    /// Clear output (only for code cells).
    pub fn clear_output(&mut self) {
        if let CellState::Code { output, .. } = self {
            *output = None;
        }
    }
}

/// Cell execution status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellStatus {
    /// Cell has not been executed.
    #[default]
    Idle,
    /// Cell is currently compiling.
    Compiling,
    /// Cell is currently executing.
    Running,
    /// Cell completed successfully.
    Success,
    /// Cell failed with an error.
    Error,
}

/// Cell output representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellOutput {
    /// Plain text representation.
    pub text: Option<String>,
    /// HTML representation.
    pub html: Option<String>,
    /// Image data (base64 encoded PNG).
    pub image: Option<String>,
    /// Structured JSON data.
    pub json: Option<serde_json::Value>,
    /// Interactive widgets defined by this cell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widgets: Vec<WidgetDef>,
}

/// Source location for error reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// Line number (1-indexed).
    pub line: u32,
    /// Column number (1-indexed).
    pub column: u32,
    /// End line (for spans).
    pub end_line: Option<u32>,
    /// End column (for spans).
    pub end_column: Option<u32>,
}

/// Compiler error information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileErrorInfo {
    /// Error message.
    pub message: String,
    /// Error code (e.g., "E0308").
    pub code: Option<String>,
    /// Source location.
    pub location: Option<SourceLocation>,
    /// Rendered error (with colors/formatting removed).
    pub rendered: Option<String>,
}

/// Dependency edge in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    /// Source cell (dependency).
    pub from: CellId,
    /// Target cell (dependent).
    pub to: CellId,
    /// Parameter name used for this dependency.
    pub param_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_serialization() {
        let msg = ClientMessage::ExecuteCell {
            cell_id: CellId::new(1),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("execute_cell"));

        let parsed: ClientMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            ClientMessage::ExecuteCell { cell_id } => {
                assert_eq!(cell_id, CellId::new(1));
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_server_message_serialization() {
        let msg = ServerMessage::CellStarted {
            cell_id: CellId::new(42),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("cell_started"));
    }

    #[test]
    fn test_cell_status_default() {
        assert_eq!(CellStatus::default(), CellStatus::Idle);
    }
}
