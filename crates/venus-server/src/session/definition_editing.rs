use super::NotebookSession;
use crate::error::{ServerError, ServerResult};
use crate::undo::UndoableOperation;
use venus_core::graph::{CellId, MoveDirection, SourceEditor};

impl NotebookSession {
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
            && std::mem::discriminant(&inferred_type) != std::mem::discriminant(&declared_type)
        {
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
                .or_else(|| {
                    // Try to find in definition cells
                    self.definition_cells
                        .iter()
                        .find(|d| d.id == id)
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
        self.undo_manager
            .record(UndoableOperation::InsertDefinitionCell {
                start_line,
                end_line,
                content: content.clone(),
                definition_type,
            });

        // Reload to update in-memory state
        self.reload()?;

        // Find the newly inserted definition cell (it should be at the expected line)
        let new_cell_id = self
            .definition_cells
            .iter()
            .find(|d| d.span.start_line >= start_line && d.span.start_line <= end_line)
            .map(|d| d.id)
            .ok_or_else(|| {
                ServerError::InvalidOperation("Failed to find inserted definition cell".to_string())
            })?;

        Ok(new_cell_id)
    }

    /// Edit a definition cell's content.
    ///
    /// Modifies the .rs source file and reloads the notebook.
    /// Returns a list of cells that are now dirty due to the definition change.
    pub fn edit_definition_cell(
        &mut self,
        cell_id: CellId,
        new_content: String,
    ) -> ServerResult<Vec<CellId>> {
        // Find the definition cell
        let def_cell = self
            .definition_cells
            .iter()
            .find(|d| d.id == cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?;

        let start_line = def_cell.span.start_line;
        let end_line = def_cell.span.end_line;
        let old_content = def_cell.content.clone();

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        // Use edit_raw_code which edits raw Rust code without // prefix
        editor.edit_raw_code(start_line, end_line, &new_content)?;
        editor.save()?;

        // Record for undo
        self.undo_manager
            .record(UndoableOperation::EditDefinitionCell {
                cell_id,
                start_line,
                end_line,
                old_content,
                new_content: new_content.clone(),
            });

        // Reload to update in-memory state (rebuilds universe with new definitions)
        self.reload()?;

        // Mark ALL executable cells as dirty (only if they have output - pristine cells stay pristine)
        let dirty_cells: Vec<CellId> = self
            .cells
            .iter()
            .filter(|c| self.cell_outputs.contains_key(&c.id)) // Only cells with output
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
        let def_cell = self
            .definition_cells
            .iter()
            .find(|d| d.id == cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?;

        let start_line = def_cell.span.start_line;
        let end_line = def_cell.span.end_line;
        let content = def_cell.content.clone();
        let definition_type = def_cell.definition_type;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.delete_markdown_cell(start_line, end_line)?;
        editor.save()?;

        // Record for undo
        self.undo_manager
            .record(UndoableOperation::DeleteDefinitionCell {
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
    pub fn move_definition_cell(
        &mut self,
        cell_id: CellId,
        direction: MoveDirection,
    ) -> ServerResult<()> {
        // Find the definition cell
        let def_cell = self
            .definition_cells
            .iter()
            .find(|d| d.id == cell_id)
            .ok_or(ServerError::CellNotFound(cell_id))?;

        let start_line = def_cell.span.start_line;
        let end_line = def_cell.span.end_line;

        // Load and edit the source file
        let mut editor = SourceEditor::load(&self.path)?;
        editor.move_markdown_cell(start_line, end_line, direction)?;
        editor.save()?;

        // Record for undo
        self.undo_manager
            .record(UndoableOperation::MoveDefinitionCell {
                start_line,
                end_line,
                direction,
            });

        // Reload to update in-memory state
        self.reload()?;

        Ok(())
    }
}
