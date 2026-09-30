//! Notebook session management.

mod code_editing;
mod definition_editing;
mod execution;
mod history;
mod lifecycle;
mod markdown_editing;
mod model;
mod state;
mod undo_redo;
mod widgets;

#[cfg(test)]
mod tests;

pub use model::{InterruptFlag, NotebookSession, OutputHistoryEntry, SessionHandle};
