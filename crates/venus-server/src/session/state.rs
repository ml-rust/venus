use super::NotebookSession;
use crate::protocol::{CellState, CellStatus, ServerMessage};
use std::collections::HashMap;
use std::path::Path;
use tokio::sync::broadcast;
use venus_core::compile::find_notebook_manifest;
use venus_core::graph::{CellId, CellInfo, CellType};

impl NotebookSession {
    /// Get the notebook path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Subscribe to server messages.
    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.tx.subscribe()
    }

    /// Get a cell by ID.
    pub(super) fn get_cell(&self, cell_id: CellId) -> Option<&CellInfo> {
        self.cells.iter().find(|c| c.id == cell_id)
    }

    /// Set the status of a code cell.
    pub(super) fn set_cell_status(&mut self, cell_id: CellId, status: CellStatus) {
        if let Some(CellState::Code {
            status: cell_status,
            ..
        }) = self.cell_states.get_mut(&cell_id)
        {
            *cell_status = status;
        }
    }

    /// Broadcast a server message, ignoring send failures.
    pub fn broadcast(&self, msg: ServerMessage) {
        let _ = self.tx.send(msg);
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
                    let remaining: Vec<&str> = lines.iter().skip(1).copied().collect();

                    // Trim leading empty lines
                    let trimmed_lines: Vec<&str> = remaining
                        .iter()
                        .skip_while(|line| line.trim().is_empty())
                        .copied()
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
    pub(super) fn update_cell_states(&mut self) {
        let mut new_states = HashMap::new();

        // Add code cells
        for cell in &self.cells {
            let existing = self.cell_states.get(&cell.id);

            // Extract status, output, dirty from existing state if it's a code cell
            let (status, output, dirty) = if let Some(CellState::Code {
                status,
                output,
                dirty,
                ..
            }) = existing
            {
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
    pub(super) fn collect_cells_in_source_order(&self) -> Vec<(CellId, usize, CellType)> {
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

        let cargo_toml_path = find_notebook_manifest(&self.path);
        let workspace_root = cargo_toml_path.as_deref().and_then(Path::parent);

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

    /// Mark a cell as dirty (needs re-execution).
    ///
    /// Only marks cells as dirty if they have existing output (data).
    /// Cells without output remain pristine (no border).
    pub fn mark_dirty(&mut self, cell_id: CellId) {
        // Mark the edited cell as dirty only if it has output
        if self.cell_outputs.contains_key(&cell_id)
            && let Some(state) = self.cell_states.get_mut(&cell_id)
        {
            state.set_dirty(true);
        }

        // Also mark dependents as dirty (only those with output)
        let dependents = self.graph.invalidated_cells(cell_id);
        for dep_id in dependents {
            if self.cell_outputs.contains_key(&dep_id)
                && let Some(state) = self.cell_states.get_mut(&dep_id)
            {
                state.set_dirty(true);
            }
        }
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
            .filter(|id| {
                self.cell_states
                    .get(id)
                    .is_some_and(|state| state.is_dirty())
            })
            .collect()
    }

    /// Get reference to cell states.
    pub fn cell_states(&self) -> &HashMap<CellId, CellState> {
        &self.cell_states
    }
}
