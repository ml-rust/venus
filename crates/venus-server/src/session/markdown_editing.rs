use super::NotebookSession;
use crate::error::{ServerError, ServerResult};
use crate::undo::UndoableOperation;
use venus_core::graph::{CellId, MoveDirection, SourceEditor};

impl NotebookSession {
    /// Insert a new markdown cell.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    pub fn insert_markdown_cell(
        &mut self,
        content: String,
        after_cell_id: Option<CellId>,
    ) -> ServerResult<()> {
        // Convert cell ID to line number if provided
        let after_line = after_cell_id.and_then(|id| {
            // Try to find in code cells
            self.cells
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.span.end_line)
                .or_else(|| {
                    // Try to find in markdown cells
                    self.markdown_cells
                        .iter()
                        .find(|m| m.id == id)
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
        self.undo_manager
            .record(UndoableOperation::InsertMarkdownCell {
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
        let md_cell = self
            .markdown_cells
            .iter()
            .find(|m| m.id == cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?;

        let start_line = md_cell.span.start_line;
        let end_line = md_cell.span.end_line;
        let old_content = md_cell.content.clone();
        let is_module_doc = md_cell.is_module_doc;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.edit_markdown_cell(start_line, end_line, &new_content, is_module_doc)?;
        editor.save()?;

        // Record for undo
        self.undo_manager
            .record(UndoableOperation::EditMarkdownCell {
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
        let md_cell = self
            .markdown_cells
            .iter()
            .find(|m| m.id == cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?;

        let start_line = md_cell.span.start_line;
        let end_line = md_cell.span.end_line;
        let content = md_cell.content.clone();

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.delete_markdown_cell(start_line, end_line)?;
        editor.save()?;

        // Record for undo
        self.undo_manager
            .record(UndoableOperation::DeleteMarkdownCell {
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
    pub fn move_markdown_cell(
        &mut self,
        cell_id: CellId,
        direction: MoveDirection,
    ) -> ServerResult<()> {
        // Find the markdown cell
        let md_cell = self
            .markdown_cells
            .iter()
            .find(|m| m.id == cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?;

        let start_line = md_cell.span.start_line;
        let end_line = md_cell.span.end_line;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.move_markdown_cell(start_line, end_line, direction)?;
        editor.save()?;

        // Record for undo
        self.undo_manager
            .record(UndoableOperation::MoveMarkdownCell {
                start_line,
                end_line,
                direction,
            });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }
}
