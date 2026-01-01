//! Undo/redo manager for cell management operations.
//!
//! Tracks operations on the notebook and allows undoing/redoing them.

use venus_core::graph::MoveDirection;

/// Maximum number of undo operations to track.
const MAX_UNDO_HISTORY: usize = 50;

/// An undoable operation on the notebook.
#[derive(Debug, Clone)]
pub enum UndoableOperation {
    /// A cell was inserted. Undo = delete this cell.
    InsertCell {
        /// Name of the inserted cell.
        cell_name: String,
        /// Name of the cell after which this was inserted (for redo).
        /// None if inserted at the beginning.
        after_cell_name: Option<String>,
    },

    /// A cell was deleted. Undo = restore it.
    DeleteCell {
        /// Name of the deleted cell.
        cell_name: String,
        /// Full source code of the cell (including doc comments and attributes).
        source: String,
        /// Name of the cell before this one (for position restoration).
        /// None if this was the first cell.
        after_cell_name: Option<String>,
    },

    /// A cell was duplicated. Undo = delete the new cell.
    DuplicateCell {
        /// Name of the original cell.
        original_cell_name: String,
        /// Name of the new duplicated cell.
        new_cell_name: String,
    },

    /// A cell was moved. Undo = move in opposite direction.
    MoveCell {
        /// Name of the moved cell.
        cell_name: String,
        /// Original direction (undo reverses it).
        direction: MoveDirection,
    },
}

impl UndoableOperation {
    /// Get a human-readable description of this operation.
    pub fn description(&self) -> String {
        match self {
            Self::InsertCell { cell_name, .. } => {
                format!("Insert cell '{}'", cell_name)
            }
            Self::DeleteCell { cell_name, .. } => {
                format!("Delete cell '{}'", cell_name)
            }
            Self::DuplicateCell { new_cell_name, .. } => {
                format!("Duplicate to '{}'", new_cell_name)
            }
            Self::MoveCell { cell_name, direction } => {
                let dir_str = match direction {
                    MoveDirection::Up => "up",
                    MoveDirection::Down => "down",
                };
                format!("Move '{}' {}", cell_name, dir_str)
            }
        }
    }

    /// Get the reverse operation (what undo would do).
    pub fn undo_description(&self) -> String {
        match self {
            Self::InsertCell { cell_name, .. } => {
                format!("Delete cell '{}'", cell_name)
            }
            Self::DeleteCell { cell_name, .. } => {
                format!("Restore cell '{}'", cell_name)
            }
            Self::DuplicateCell { new_cell_name, .. } => {
                format!("Delete cell '{}'", new_cell_name)
            }
            Self::MoveCell { cell_name, direction } => {
                let dir_str = match direction {
                    MoveDirection::Up => "down",
                    MoveDirection::Down => "up",
                };
                format!("Move '{}' {}", cell_name, dir_str)
            }
        }
    }
}

/// Manages undo/redo stacks for cell operations.
#[derive(Debug, Default)]
pub struct UndoManager {
    /// Stack of operations that can be undone.
    undo_stack: Vec<UndoableOperation>,
    /// Stack of operations that can be redone.
    redo_stack: Vec<UndoableOperation>,
}

impl UndoManager {
    /// Create a new undo manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an operation that was just performed.
    ///
    /// This clears the redo stack (can't redo after a new operation).
    pub fn record(&mut self, operation: UndoableOperation) {
        // Clear redo stack - new operation invalidates redo history
        self.redo_stack.clear();

        // Add to undo stack
        self.undo_stack.push(operation);

        // Trim if too long
        while self.undo_stack.len() > MAX_UNDO_HISTORY {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the last operation from the undo stack.
    ///
    /// Returns the operation to undo, or None if stack is empty.
    /// The caller should execute the reverse operation, then call `record_redo`.
    pub fn pop_undo(&mut self) -> Option<UndoableOperation> {
        self.undo_stack.pop()
    }

    /// Record an operation that was just undone (for redo).
    pub fn record_redo(&mut self, operation: UndoableOperation) {
        self.redo_stack.push(operation);
    }

    /// Pop the last operation from the redo stack.
    ///
    /// Returns the operation to redo, or None if stack is empty.
    /// The caller should execute the operation, then call `record` as normal.
    pub fn pop_redo(&mut self) -> Option<UndoableOperation> {
        self.redo_stack.pop()
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Get description of what will be undone (for UI).
    pub fn undo_description(&self) -> Option<String> {
        self.undo_stack.last().map(|op| op.undo_description())
    }

    /// Get description of what will be redone (for UI).
    pub fn redo_description(&self) -> Option<String> {
        self.redo_stack.last().map(|op| op.description())
    }

    /// Clear all undo/redo history.
    ///
    /// Called when the file is externally modified.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_undo() {
        let mut manager = UndoManager::new();

        // Record an insert
        manager.record(UndoableOperation::InsertCell {
            cell_name: "test_cell".to_string(),
            after_cell_name: None,
        });

        assert!(manager.can_undo());
        assert!(!manager.can_redo());

        // Undo it
        let op = manager.pop_undo().unwrap();
        assert!(matches!(op, UndoableOperation::InsertCell { .. }));

        // Record for redo
        manager.record_redo(op);

        assert!(!manager.can_undo());
        assert!(manager.can_redo());
    }

    #[test]
    fn test_new_operation_clears_redo() {
        let mut manager = UndoManager::new();

        // Record and undo
        manager.record(UndoableOperation::InsertCell {
            cell_name: "cell1".to_string(),
            after_cell_name: None,
        });
        let op = manager.pop_undo().unwrap();
        manager.record_redo(op);

        assert!(manager.can_redo());

        // New operation should clear redo
        manager.record(UndoableOperation::InsertCell {
            cell_name: "cell2".to_string(),
            after_cell_name: Some("cell1".to_string()),
        });

        assert!(!manager.can_redo());
    }

    #[test]
    fn test_descriptions() {
        let op = UndoableOperation::DeleteCell {
            cell_name: "foo".to_string(),
            source: "".to_string(),
            after_cell_name: None,
        };

        assert_eq!(op.description(), "Delete cell 'foo'");
        assert_eq!(op.undo_description(), "Restore cell 'foo'");
    }

    #[test]
    fn test_move_descriptions() {
        let op = UndoableOperation::MoveCell {
            cell_name: "bar".to_string(),
            direction: MoveDirection::Up,
        };

        assert_eq!(op.description(), "Move 'bar' up");
        assert_eq!(op.undo_description(), "Move 'bar' down");
    }
}
