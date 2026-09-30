use super::NotebookSession;
use crate::error::{ServerError, ServerResult};
use crate::protocol::ServerMessage;
use crate::undo::UndoableOperation;
use venus_core::graph::{MoveDirection, SourceEditor};

impl NotebookSession {
    /// Undo the last cell management operation.
    ///
    /// Returns a description of what was undone, or an error if undo failed.
    pub fn undo(&mut self) -> ServerResult<String> {
        let operation = self
            .undo_manager
            .pop_undo()
            .ok_or_else(|| ServerError::InvalidOperation("Nothing to undo".to_string()))?;

        let description = operation.undo_description();

        // Execute the reverse operation
        let mut editor = SourceEditor::load(&self.path)?;

        match &operation {
            UndoableOperation::InsertCell { cell_name, .. } => {
                // Undo insert = delete
                editor.delete_cell(cell_name)?;
            }
            UndoableOperation::DeleteCell {
                source,
                after_cell_name,
                ..
            } => {
                // Undo delete = restore
                editor.restore_cell(source, after_cell_name.as_deref())?;
            }
            UndoableOperation::DuplicateCell { new_cell_name, .. } => {
                // Undo duplicate = delete the new cell
                editor.delete_cell(new_cell_name)?;
            }
            UndoableOperation::MoveCell {
                cell_name,
                direction,
            } => {
                // Undo move = move in opposite direction
                let reverse_direction = match direction {
                    MoveDirection::Up => MoveDirection::Down,
                    MoveDirection::Down => MoveDirection::Up,
                };
                editor.move_cell(cell_name, reverse_direction)?;
            }
            UndoableOperation::RenameCell {
                cell_name,
                old_display_name,
                ..
            } => {
                // Undo rename = restore old display name
                editor.rename_cell(cell_name, old_display_name)?;
            }
            UndoableOperation::EditCell {
                start_line,
                end_line,
                old_source,
                ..
            } => {
                // Undo edit = restore old source
                editor.edit_raw_code(*start_line, *end_line, old_source)?;
            }
            UndoableOperation::InsertMarkdownCell {
                start_line,
                end_line,
                ..
            } => {
                // Undo insert markdown = delete it
                editor.delete_markdown_cell(*start_line, *end_line)?;
            }
            UndoableOperation::EditMarkdownCell {
                start_line,
                end_line,
                old_content,
                is_module_doc,
                ..
            } => {
                // Undo edit markdown = restore old content
                editor.edit_markdown_cell(*start_line, *end_line, old_content, *is_module_doc)?;
            }
            UndoableOperation::DeleteMarkdownCell {
                start_line,
                content,
            } => {
                // Undo delete markdown = restore it
                let after_line = if *start_line > 0 {
                    Some(start_line - 1)
                } else {
                    None
                };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::MoveMarkdownCell {
                start_line,
                end_line,
                direction,
            } => {
                // Undo move markdown = move in opposite direction
                let reverse_direction = match direction {
                    MoveDirection::Up => MoveDirection::Down,
                    MoveDirection::Down => MoveDirection::Up,
                };
                editor.move_markdown_cell(*start_line, *end_line, reverse_direction)?;
            }
            UndoableOperation::InsertDefinitionCell {
                start_line,
                end_line,
                ..
            } => {
                // Undo insert definition = delete it
                editor.delete_markdown_cell(*start_line, *end_line)?;
            }
            UndoableOperation::EditDefinitionCell {
                start_line,
                end_line,
                old_content,
                ..
            } => {
                // Undo edit definition = restore old content
                editor.edit_markdown_cell(*start_line, *end_line, old_content, false)?;
            }
            UndoableOperation::DeleteDefinitionCell {
                start_line,
                content,
                ..
            } => {
                // Undo delete definition = restore it
                let after_line = if *start_line > 0 {
                    Some(start_line - 1)
                } else {
                    None
                };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::MoveDefinitionCell {
                start_line,
                end_line,
                direction,
            } => {
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
        let operation = self
            .undo_manager
            .pop_redo()
            .ok_or_else(|| ServerError::InvalidOperation("Nothing to redo".to_string()))?;

        let description = operation.description();

        // Execute the original operation
        let mut editor = SourceEditor::load(&self.path)?;

        match &operation {
            UndoableOperation::InsertCell {
                after_cell_name, ..
            } => {
                // Re-insert at the original position
                let _ = editor.insert_cell(after_cell_name.as_deref())?;
            }
            UndoableOperation::DeleteCell { cell_name, .. } => {
                // Redo delete = delete again
                editor.delete_cell(cell_name)?;
            }
            UndoableOperation::DuplicateCell {
                original_cell_name, ..
            } => {
                // Redo duplicate = duplicate again (new name will be generated)
                let _ = editor.duplicate_cell(original_cell_name)?;
            }
            UndoableOperation::MoveCell {
                cell_name,
                direction,
            } => {
                // Redo move = move in same direction
                editor.move_cell(cell_name, *direction)?;
            }
            UndoableOperation::RenameCell {
                cell_name,
                new_display_name,
                ..
            } => {
                // Redo rename = apply new display name again
                editor.rename_cell(cell_name, new_display_name)?;
            }
            UndoableOperation::EditCell {
                start_line,
                end_line,
                new_source,
                ..
            } => {
                // Redo edit = apply new source again
                editor.edit_raw_code(*start_line, *end_line, new_source)?;
            }
            UndoableOperation::InsertMarkdownCell {
                start_line,
                content,
                ..
            } => {
                // Redo insert markdown = insert again at original position
                let after_line = if *start_line > 0 {
                    Some(start_line - 1)
                } else {
                    None
                };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::EditMarkdownCell {
                start_line,
                end_line,
                new_content,
                is_module_doc,
                ..
            } => {
                // Redo edit markdown = apply new content again
                editor.edit_markdown_cell(*start_line, *end_line, new_content, *is_module_doc)?;
            }
            UndoableOperation::DeleteMarkdownCell {
                start_line,
                content,
            } => {
                // Redo delete markdown = delete again
                // We need to find the end line by counting content lines
                let line_count = content.lines().count();
                let end_line = start_line + line_count;
                editor.delete_markdown_cell(*start_line, end_line)?;
            }
            UndoableOperation::MoveMarkdownCell {
                start_line,
                end_line,
                direction,
            } => {
                // Redo move markdown = move in same direction
                editor.move_markdown_cell(*start_line, *end_line, *direction)?;
            }
            UndoableOperation::InsertDefinitionCell {
                start_line,
                content,
                ..
            } => {
                // Redo insert definition = insert again at original position
                let after_line = if *start_line > 0 {
                    Some(start_line - 1)
                } else {
                    None
                };
                editor.insert_markdown_cell(content, after_line)?;
            }
            UndoableOperation::EditDefinitionCell {
                start_line,
                end_line,
                new_content,
                ..
            } => {
                // Redo edit definition = apply new content again
                editor.edit_markdown_cell(*start_line, *end_line, new_content, false)?;
            }
            UndoableOperation::DeleteDefinitionCell {
                start_line,
                content,
                ..
            } => {
                // Redo delete definition = delete again
                let line_count = content.lines().count();
                let end_line = start_line + line_count;
                editor.delete_markdown_cell(*start_line, end_line)?;
            }
            UndoableOperation::MoveDefinitionCell {
                start_line,
                end_line,
                direction,
            } => {
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
