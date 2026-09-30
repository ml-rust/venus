use super::NotebookSession;
use super::model::{MAX_HISTORY_PER_CELL, OutputHistoryEntry};
use crate::protocol::{CellOutput, CellStatus, ServerMessage};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use venus_core::graph::CellId;
use venus_core::state::BoxedOutput;

impl NotebookSession {
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

    /// Add an execution result to history.
    pub(super) fn add_to_history(
        &mut self,
        cell_id: CellId,
        serialized: Arc<BoxedOutput>,
        display: CellOutput,
    ) {
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
        self.executor
            .state_mut()
            .store_output(cell_id, (*serialized).clone());

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
    pub(super) fn mark_dependents_dirty_and_get(&mut self, cell_id: CellId) -> Vec<CellId> {
        // Use the graph's invalidated_cells which returns all dependents
        let dependents = self.graph.invalidated_cells(cell_id);
        let mut dirty_cells = Vec::new();

        // Skip the first one (the changed cell itself) and mark the rest as dirty
        // BUT only if they have output (data) - pristine cells stay pristine
        for dep_id in dependents.into_iter().skip(1) {
            // Only mark dirty if cell has output (has been executed before)
            if self.cell_outputs.contains_key(&dep_id)
                && let Some(state) = self.cell_states.get_mut(&dep_id)
            {
                state.set_dirty(true);
                dirty_cells.push(dep_id);
            }
        }
        dirty_cells
    }

    /// Compute a hash of output bytes for change detection.
    pub(super) fn output_hash(output: &BoxedOutput) -> u64 {
        let mut hasher = rustc_hash::FxHasher::default();
        output.bytes().hash(&mut hasher);
        hasher.finish()
    }

    /// Get history count for a cell.
    pub fn get_history_count(&self, cell_id: CellId) -> usize {
        self.cell_output_history
            .get(&cell_id)
            .map(|h| h.len())
            .unwrap_or(0)
    }

    /// Get current history index for a cell.
    pub fn get_history_index(&self, cell_id: CellId) -> usize {
        self.cell_history_index.get(&cell_id).copied().unwrap_or(0)
    }
}
