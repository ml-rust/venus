use crate::protocol::{CellOutput, CellState, ServerMessage};
use crate::undo::UndoManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;
use tokio::sync::{RwLock, broadcast};
use venus_core::compile::{CompilerConfig, ToolchainManager};
use venus_core::execute::ProcessExecutor;
use venus_core::graph::{CellId, CellInfo, DefinitionCell, GraphEngine, MarkdownCell};
use venus_core::state::BoxedOutput;
use venus_core::widgets::{WidgetDef, WidgetValue};

/// Shared interrupt flag that can be checked without locks.
pub type InterruptFlag = Arc<AtomicBool>;

/// Capacity for the broadcast channel.
/// 256 messages should be sufficient for normal notebook operation.
/// If clients fall behind, older messages will be dropped.
pub(super) const MESSAGE_CHANNEL_CAPACITY: usize = 256;

/// A notebook session.
pub struct NotebookSession {
    /// Path to the notebook file.
    pub(super) path: PathBuf,

    /// Parsed code cells.
    pub(super) cells: Vec<CellInfo>,

    /// Parsed markdown cells.
    pub(super) markdown_cells: Vec<MarkdownCell>,

    /// Parsed definition cells (imports, types, helpers).
    pub(super) definition_cells: Vec<DefinitionCell>,

    /// Dependency graph.
    pub(super) graph: GraphEngine,

    /// Cell states for clients (both code and markdown).
    pub(super) cell_states: HashMap<CellId, CellState>,

    /// Toolchain manager.
    pub(super) toolchain: ToolchainManager,

    /// Compiler configuration.
    pub(super) config: CompilerConfig,

    /// Universe path (compiled dependencies).
    pub(super) universe_path: Option<PathBuf>,

    /// Dependencies hash for cache invalidation.
    pub(super) deps_hash: u64,

    /// Broadcast channel for server messages.
    pub(super) tx: broadcast::Sender<ServerMessage>,

    /// Whether an execution is in progress.
    pub(super) executing: bool,

    /// Cached cell outputs for dependency passing.
    /// Maps cell ID to its serialized output.
    pub(super) cell_outputs: HashMap<CellId, Arc<BoxedOutput>>,

    /// Process-based executor for isolated cell execution.
    /// Uses worker processes that can be killed for true interruption.
    pub(super) executor: ProcessExecutor,

    /// Optional execution timeout for execute_all.
    /// After this duration, the executor kills the current worker.
    pub(super) execution_timeout: Option<Duration>,

    /// Shared flag indicating if current execution was interrupted by user.
    /// When true, errors should be reported as "interrupted" not as failures.
    /// This is shared with AppState so interrupt handler can set it.
    pub(super) interrupted: InterruptFlag,

    /// Widget values per cell.
    /// Maps cell ID -> widget ID -> current value.
    pub(super) widget_values: HashMap<CellId, HashMap<String, WidgetValue>>,

    /// Widget definitions per cell (from last execution).
    /// Used to send widget state to newly connected clients.
    pub(super) widget_defs: HashMap<CellId, Vec<WidgetDef>>,

    /// Execution history per cell.
    /// Stores both serialized output (for dependent cells) and display output.
    pub(super) cell_output_history: HashMap<CellId, Vec<OutputHistoryEntry>>,

    /// Current history index per cell.
    pub(super) cell_history_index: HashMap<CellId, usize>,

    /// Undo/redo manager for cell operations.
    pub(super) undo_manager: UndoManager,

    /// Pending edits from the editor (not yet saved to disk).
    /// These are saved to disk when the cell is executed.
    pub(super) pending_edits: HashMap<CellId, String>,
}

/// Maximum number of history entries per cell.
pub(super) const MAX_HISTORY_PER_CELL: usize = 10;

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
