//! Notebook directory management.
//!
//! Provides consistent directory structure for Venus notebooks,
//! ensuring the same paths are used across CLI and server components.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Directory structure for a Venus notebook.
///
/// All Venus-related files are stored under a `.venus` directory
/// next to the notebook file:
///
/// ```text
/// notebook.rs
/// .venus/
/// ├── build/      # Compiled cell dylibs
/// │   ├── cells/  # Individual cell builds
/// │   └── universe/ # Universe library build
/// ├── cache/      # Compilation cache metadata
/// └── state/      # Persistent cell outputs
/// ```
#[derive(Debug, Clone)]
pub struct NotebookDirs {
    /// The `.venus` directory itself.
    pub venus_dir: PathBuf,

    /// Build directory for compiled artifacts.
    pub build_dir: PathBuf,

    /// Cache directory for compilation metadata.
    pub cache_dir: PathBuf,

    /// State directory for persistent outputs.
    pub state_dir: PathBuf,
}

impl NotebookDirs {
    /// Create directory structure from a notebook path.
    ///
    /// Creates all necessary directories if they don't exist.
    ///
    /// # Arguments
    /// * `notebook_path` - Path to the notebook file (e.g., `examples/hello.rs`)
    ///
    /// # Errors
    /// Returns an error if directory creation fails.
    pub fn from_notebook_path(notebook_path: &Path) -> Result<Self> {
        let notebook_dir = notebook_path.parent().unwrap_or(Path::new("."));
        Self::from_notebook_dir(notebook_dir)
    }

    /// Create directory structure from the notebook's parent directory.
    ///
    /// Creates all necessary directories if they don't exist.
    ///
    /// # Arguments
    /// * `notebook_dir` - Directory containing the notebook file
    ///
    /// # Errors
    /// Returns an error if directory creation fails.
    pub fn from_notebook_dir(notebook_dir: &Path) -> Result<Self> {
        let venus_dir = notebook_dir.join(".venus");
        let build_dir = venus_dir.join("build");
        let cache_dir = venus_dir.join("cache");
        let state_dir = venus_dir.join("state");

        // Create all directories (Error::Io auto-converts via #[from])
        fs::create_dir_all(&build_dir)?;
        fs::create_dir_all(&cache_dir)?;
        fs::create_dir_all(&state_dir)?;

        Ok(Self {
            venus_dir,
            build_dir,
            cache_dir,
            state_dir,
        })
    }

    /// Clean all build artifacts.
    ///
    /// Removes the entire `.venus` directory and recreates it.
    pub fn clean(&self) -> Result<()> {
        if self.venus_dir.exists() {
            fs::remove_dir_all(&self.venus_dir)?;
        }

        // Recreate the structure
        fs::create_dir_all(&self.build_dir)?;
        fs::create_dir_all(&self.cache_dir)?;
        fs::create_dir_all(&self.state_dir)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_from_notebook_path() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let notebook_path = temp.path().join("test.rs");

        let dirs = NotebookDirs::from_notebook_path(&notebook_path)
            .expect("Failed to create dirs");

        assert!(dirs.venus_dir.ends_with(".venus"));
        assert!(dirs.build_dir.exists());
        assert!(dirs.cache_dir.exists());
        assert!(dirs.state_dir.exists());
    }

    #[test]
    fn test_clean() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let notebook_path = temp.path().join("test.rs");

        let dirs = NotebookDirs::from_notebook_path(&notebook_path)
            .expect("Failed to create dirs");

        // Create a test file
        let test_file = dirs.build_dir.join("test.txt");
        fs::write(&test_file, "test").expect("Failed to write test file");
        assert!(test_file.exists());

        // Clean should remove everything
        dirs.clean().expect("Failed to clean");
        assert!(!test_file.exists());

        // But directories should be recreated
        assert!(dirs.build_dir.exists());
    }
}
