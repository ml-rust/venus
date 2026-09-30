use super::NotebookSession;
use crate::error::{ServerError, ServerResult};
use crate::undo::UndoableOperation;
use std::collections::HashMap;
use std::sync::Arc;
use venus_core::graph::{CellId, MoveDirection, SourceEditor};
use venus_core::state::BoxedOutput;

impl NotebookSession {
    /// Insert a new cell after the specified cell.
    ///
    /// Modifies the source file and triggers a reload.
    /// Returns the name of the newly created cell.
    pub fn insert_cell(&mut self, after_cell_id: Option<CellId>) -> ServerResult<String> {
        // Convert CellId to cell name if provided
        let after_name = after_cell_id.and_then(|id| {
            self.cells
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
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
        let cell_name = self
            .cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| c.name.clone())
            .ok_or(ServerError::CellNotFound(cell_id))?;

        // Check if any other cells depend on this cell
        let dependents: Vec<String> = self
            .cells
            .iter()
            .filter(|c| c.id != cell_id) // Don't check self
            .filter(|c| c.dependencies.iter().any(|dep| dep.param_name == cell_name))
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
        let cell_name = self
            .cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| c.name.clone())
            .ok_or(ServerError::CellNotFound(cell_id))?;

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
        let cell_name = self
            .cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| c.name.clone())
            .ok_or(ServerError::CellNotFound(cell_id))?;

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
        let cell = self
            .cells
            .iter()
            .find(|c| c.id == cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?;

        let cell_name = cell.name.clone();
        let old_source = cell.source_code.clone();

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;

        // Reconstruct complete cell (doc comments + #[venus::cell] + function) and get FRESH line numbers
        let (reconstructed, start_line, end_line) =
            editor.reconstruct_and_get_span(&cell_name, &new_source)?;

        tracing::info!(
            "Editing cell '{}' lines {}-{}, reconstructed length: {}",
            cell_name,
            start_line,
            end_line,
            reconstructed.len()
        );
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
        let outputs_by_name: HashMap<String, Arc<BoxedOutput>> = self
            .cells
            .iter()
            .filter_map(|c| {
                self.cell_outputs
                    .get(&c.id)
                    .map(|o| (c.name.clone(), o.clone()))
            })
            .collect();

        self.reload()?;

        // Restore outputs with NEW IDs (except for the edited cell)
        self.cell_outputs.clear();
        for cell in &self.cells {
            if cell.name != cell_name
                && let Some(output) = outputs_by_name.get(&cell.name)
            {
                self.cell_outputs.insert(cell.id, output.clone());
            }
        }

        Ok(())
    }

    /// Rename a cell's display name.
    ///
    /// Updates the cell's doc comment with the new display name and reloads the notebook.
    pub fn rename_cell(&mut self, cell_id: CellId, new_display_name: String) -> ServerResult<()> {
        // Find the cell name and current display name
        let (cell_name, old_display_name) = self
            .cells
            .iter()
            .find(|c| c.id == cell_id)
            .map(|c| (c.name.clone(), c.display_name.clone()))
            .ok_or(ServerError::CellNotFound(cell_id))?;

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
}
