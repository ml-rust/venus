//! File watcher for detecting notebook changes.
//!
//! Watches `.rs` notebook files and notifies the session when changes occur.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};
use tokio::sync::mpsc;

use crate::error::{ServerError, ServerResult};

/// File change event.
#[derive(Debug, Clone)]
pub enum FileEvent {
    /// File was modified.
    Modified(PathBuf),
    /// File was created.
    Created(PathBuf),
    /// File was removed.
    Removed(PathBuf),
}

/// File watcher handle.
pub struct FileWatcher {
    /// Debouncer handle (kept alive to maintain watcher).
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    /// Receiver for file events.
    rx: mpsc::UnboundedReceiver<FileEvent>,
}

impl FileWatcher {
    /// Create a new file watcher for the given path.
    pub fn new(path: impl AsRef<Path>) -> ServerResult<Self> {
        let path = path.as_ref().to_path_buf();
        let watch_path = if path.is_file() {
            path.parent().unwrap_or(Path::new(".")).to_path_buf()
        } else {
            path.clone()
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let target_file = if path.is_file() {
            Some(Arc::new(path))
        } else {
            None
        };

        let mut debouncer = new_debouncer(
            Duration::from_millis(200),
            move |result: DebounceEventResult| {
                if let Ok(events) = result {
                    for event in events {
                        let event_path = &event.path;

                        // Filter to only .rs files
                        if event_path.extension().is_none_or(|ext| ext != "rs") {
                            continue;
                        }

                        // If watching a specific file, only report events for that file
                        if let Some(ref target) = target_file
                            && event_path != target.as_ref()
                        {
                            continue;
                        }

                        let file_event = if event_path.exists() {
                            FileEvent::Modified(event_path.clone())
                        } else {
                            FileEvent::Removed(event_path.clone())
                        };

                        let _ = tx.send(file_event);
                    }
                }
            },
        )
        .map_err(|e| ServerError::Watch(e.to_string()))?;

        debouncer
            .watcher()
            .watch(&watch_path, RecursiveMode::NonRecursive)
            .map_err(|e| ServerError::Watch(e.to_string()))?;

        Ok(Self {
            _debouncer: debouncer,
            rx,
        })
    }

    /// Receive the next file event.
    pub async fn recv(&mut self) -> Option<FileEvent> {
        self.rx.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_watcher_creation() {
        let temp = TempDir::new().unwrap();
        let notebook = temp.path().join("test.rs");
        fs::write(&notebook, "// test").unwrap();

        let watcher = FileWatcher::new(&notebook);
        assert!(watcher.is_ok());
    }
}
