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
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_watcher_creation() {
        let temp = TempDir::new().unwrap();
        let notebook = temp.path().join("test.rs");
        fs::write(&notebook, "// test").unwrap();

        let watcher = FileWatcher::new(&notebook);
        assert!(watcher.is_ok());
    }

    #[tokio::test]
    async fn test_watcher_detects_modification() {
        let temp = TempDir::new().unwrap();
        let notebook = temp.path().join("test.rs");
        fs::write(&notebook, "// initial content").unwrap();

        let mut watcher = FileWatcher::new(&notebook).unwrap();

        // Give the watcher time to initialize
        sleep(Duration::from_millis(100)).await;

        // Modify the file
        fs::write(&notebook, "// modified content").unwrap();

        // Wait for debounce + processing
        let timeout = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

        assert!(timeout.is_ok(), "Watcher did not detect modification");
        let event = timeout.unwrap();

        match event {
            Some(FileEvent::Modified(path)) => {
                assert_eq!(path, notebook);
            }
            Some(other) => panic!("Expected Modified event, got {:?}", other),
            None => panic!("Received None from watcher"),
        }
    }

    #[tokio::test]
    async fn test_watcher_ignores_non_rust_files() {
        let temp = TempDir::new().unwrap();
        let notebook = temp.path().join("test.rs");
        fs::write(&notebook, "// test").unwrap();

        let mut watcher = FileWatcher::new(&notebook).unwrap();

        // Give the watcher time to initialize
        sleep(Duration::from_millis(100)).await;

        // Create a non-Rust file (should be ignored)
        let other_file = temp.path().join("test.txt");
        fs::write(&other_file, "text content").unwrap();

        // Wait a bit to ensure no event is generated
        let timeout =
            tokio::time::timeout(Duration::from_millis(500), watcher.recv()).await;

        // Should timeout because .txt files are filtered out
        assert!(timeout.is_err(), "Watcher should ignore non-.rs files");
    }

    #[tokio::test]
    async fn test_watcher_directory_mode() {
        let temp = TempDir::new().unwrap();
        let notebook = temp.path().join("test.rs");
        fs::write(&notebook, "// test").unwrap();

        // Watch the directory instead of specific file
        let mut watcher = FileWatcher::new(temp.path()).unwrap();

        // Give the watcher time to initialize
        sleep(Duration::from_millis(100)).await;

        // Modify the file
        fs::write(&notebook, "// modified").unwrap();

        // Should still detect changes
        let timeout = tokio::time::timeout(Duration::from_secs(2), watcher.recv()).await;

        assert!(
            timeout.is_ok(),
            "Directory watcher did not detect file modification"
        );
    }

    #[tokio::test]
    async fn test_file_event_types() {
        // Test FileEvent variants
        let event = FileEvent::Modified(PathBuf::from("/test.rs"));
        assert!(matches!(event, FileEvent::Modified(_)));

        let event = FileEvent::Created(PathBuf::from("/test.rs"));
        assert!(matches!(event, FileEvent::Created(_)));

        let event = FileEvent::Removed(PathBuf::from("/test.rs"));
        assert!(matches!(event, FileEvent::Removed(_)));
    }
}
