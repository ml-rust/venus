//! Windows DLL hot-reload handler.
//!
//! On Windows, loaded DLLs cannot be deleted or overwritten while in use.
//! This module provides a UUID-based strategy to work around this limitation:
//!
//! 1. When loading a DLL, copy it to a unique temp file with UUID suffix
//! 2. Load the copy instead of the original
//! 3. Track which copies are in use
//! 4. Clean up old copies when safe
//!
//! # Example
//!
//! ```ignore
//! use venus_core::execute::WindowsDllHandler;
//!
//! let mut handler = WindowsDllHandler::new(PathBuf::from(".venus/build/temp"));
//!
//! // Load a DLL (on Windows, this creates a UUID copy)
//! let loadable_path = handler.prepare_for_load(&compiled_dll_path)?;
//! let library = Library::new(&loadable_path)?;
//!
//! // When done with the library (must drop it first)
//! drop(library);
//! handler.release(&loadable_path);
//!
//! // Clean up old copies
//! handler.cleanup_old_copies()?;
//! ```

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[cfg(windows)]
use uuid::Uuid;

/// Handler for Windows DLL hot-reload.
///
/// Manages UUID-named copies of DLLs to allow recompilation while
/// previous versions are still loaded.
pub struct WindowsDllHandler {
    /// Directory for temporary DLL copies.
    temp_dir: PathBuf,

    /// Map from temp path to original path for tracking.
    /// Key: UUID-named temp path, Value: original DLL path
    active_copies: HashMap<PathBuf, PathBuf>,

    /// Maximum age for unused DLL copies before cleanup (default: 1 hour).
    max_age: Duration,
}

impl WindowsDllHandler {
    /// Create a new Windows DLL handler.
    ///
    /// # Arguments
    ///
    /// * `temp_dir` - Directory for temporary DLL copies
    pub fn new(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir,
            active_copies: HashMap::new(),
            max_age: Duration::from_secs(3600), // 1 hour default
        }
    }

    /// Set the maximum age for cleanup.
    pub fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = max_age;
        self
    }

    /// Prepare a DLL for loading.
    ///
    /// On Windows, this copies the DLL to a UUID-named file in the temp directory.
    /// On other platforms, this returns the original path unchanged.
    ///
    /// # Arguments
    ///
    /// * `dll_path` - Path to the compiled DLL
    ///
    /// # Returns
    ///
    /// Path to load (either the original or a UUID copy).
    pub fn prepare_for_load(&mut self, dll_path: &Path) -> io::Result<PathBuf> {
        #[cfg(windows)]
        {
            self.create_uuid_copy(dll_path)
        }

        #[cfg(not(windows))]
        {
            // On non-Windows platforms, just return the original path
            Ok(dll_path.to_path_buf())
        }
    }

    /// Create a UUID-named copy of a DLL (Windows-specific).
    #[cfg(windows)]
    fn create_uuid_copy(&mut self, dll_path: &Path) -> io::Result<PathBuf> {
        // Ensure temp directory exists
        fs::create_dir_all(&self.temp_dir)?;

        // Generate UUID-based filename
        let uuid = Uuid::new_v4();
        let original_name = dll_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("cell");
        let extension = dll_path.extension().and_then(|s| s.to_str()).unwrap_or("dll");

        let temp_name = format!("{}-{}.{}", original_name, uuid, extension);
        let temp_path = self.temp_dir.join(temp_name);

        // Copy the DLL
        fs::copy(dll_path, &temp_path)?;

        // Track the copy
        self.active_copies
            .insert(temp_path.clone(), dll_path.to_path_buf());

        tracing::debug!(
            "Created DLL copy: {} -> {}",
            dll_path.display(),
            temp_path.display()
        );

        Ok(temp_path)
    }

    /// Release a loaded DLL path.
    ///
    /// Call this after dropping the loaded library to mark the temp file
    /// as eligible for cleanup.
    ///
    /// # Arguments
    ///
    /// * `loaded_path` - Path that was returned by `prepare_for_load`
    pub fn release(&mut self, loaded_path: &Path) {
        self.active_copies.remove(loaded_path);
    }

    /// Check if a path is an active copy.
    pub fn is_active(&self, path: &Path) -> bool {
        self.active_copies.contains_key(path)
    }

    /// Get all active copy paths.
    pub fn active_paths(&self) -> impl Iterator<Item = &Path> {
        self.active_copies.keys().map(|p| p.as_path())
    }

    /// Clean up old DLL copies.
    ///
    /// Removes temp files that are:
    /// 1. Not currently tracked as active
    /// 2. Older than `max_age`
    ///
    /// # Returns
    ///
    /// Number of files cleaned up.
    pub fn cleanup_old_copies(&self) -> io::Result<usize> {
        if !self.temp_dir.exists() {
            return Ok(0);
        }

        let cutoff = SystemTime::now()
            .checked_sub(self.max_age)
            .unwrap_or(SystemTime::UNIX_EPOCH);

        let mut cleaned = 0;

        for entry in fs::read_dir(&self.temp_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Skip if this is an active copy
            if self.active_copies.contains_key(&path) {
                continue;
            }

            // Check file age - collapse nested conditionals for clarity
            let is_old = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|modified| modified < cutoff)
                .unwrap_or(false);

            if is_old && fs::remove_file(&path).is_ok() {
                tracing::debug!("Cleaned up old DLL: {}", path.display());
                cleaned += 1;
            }
        }

        if cleaned > 0 {
            tracing::info!("Cleaned up {} old DLL copies", cleaned);
        }

        Ok(cleaned)
    }

    /// Force cleanup of all non-active copies.
    ///
    /// Attempts to remove all temp files that are not currently active.
    /// Files that are locked will be skipped.
    ///
    /// # Returns
    ///
    /// Number of files cleaned up.
    pub fn cleanup_all(&self) -> io::Result<usize> {
        if !self.temp_dir.exists() {
            return Ok(0);
        }

        let mut cleaned = 0;

        for entry in fs::read_dir(&self.temp_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Skip if this is an active copy
            if self.active_copies.contains_key(&path) {
                continue;
            }

            // Skip non-DLL files
            let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if !matches!(extension, "dll" | "so" | "dylib") {
                continue;
            }

            // Try to remove (may fail on Windows if still locked)
            if let Ok(()) = fs::remove_file(&path) {
                tracing::debug!("Force cleaned DLL: {}", path.display());
                cleaned += 1;
            }
        }

        Ok(cleaned)
    }

    /// Get the temp directory path.
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }
}

impl Default for WindowsDllHandler {
    fn default() -> Self {
        Self::new(PathBuf::from(".venus/build/temp"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use tempfile::tempdir;

    #[test]
    fn test_handler_creation() {
        let handler = WindowsDllHandler::new(PathBuf::from("/tmp/test"));
        assert_eq!(handler.temp_dir(), Path::new("/tmp/test"));
        assert!(handler.active_paths().next().is_none());
    }

    #[test]
    fn test_with_max_age() {
        let handler = WindowsDllHandler::new(PathBuf::from("/tmp/test"))
            .with_max_age(Duration::from_secs(60));
        assert_eq!(handler.max_age, Duration::from_secs(60));
    }

    #[test]
    fn test_prepare_for_load_non_windows() {
        let temp = tempdir().unwrap();
        let mut handler = WindowsDllHandler::new(temp.path().join("temp"));

        let dll_path = temp.path().join("test.so");
        fs::write(&dll_path, b"fake dll").unwrap();

        let result = handler.prepare_for_load(&dll_path).unwrap();

        // On non-Windows, should return original path
        #[cfg(not(windows))]
        assert_eq!(result, dll_path);

        // On Windows, should return a UUID copy
        #[cfg(windows)]
        {
            assert_ne!(result, dll_path);
            assert!(result.exists());
            assert!(handler.is_active(&result));
        }
    }

    #[test]
    fn test_release() {
        let temp = tempdir().unwrap();
        let mut handler = WindowsDllHandler::new(temp.path().join("temp"));

        let fake_path = temp.path().join("fake.dll");
        handler.active_copies.insert(fake_path.clone(), PathBuf::from("original.dll"));

        assert!(handler.is_active(&fake_path));
        handler.release(&fake_path);
        assert!(!handler.is_active(&fake_path));
    }

    #[test]
    fn test_cleanup_old_copies() {
        let temp = tempdir().unwrap();
        let temp_dir = temp.path().join("temp");
        fs::create_dir_all(&temp_dir).unwrap();

        let handler = WindowsDllHandler::new(temp_dir.clone())
            .with_max_age(Duration::from_millis(10));

        // Create an old file
        let old_file = temp_dir.join("old-test.dll");
        fs::write(&old_file, b"old").unwrap();

        // Wait for it to age
        thread::sleep(Duration::from_millis(20));

        // Clean up
        let cleaned = handler.cleanup_old_copies().unwrap();

        assert_eq!(cleaned, 1);
        assert!(!old_file.exists());
    }

    #[test]
    fn test_cleanup_skips_active() {
        let temp = tempdir().unwrap();
        let temp_dir = temp.path().join("temp");
        fs::create_dir_all(&temp_dir).unwrap();

        let mut handler = WindowsDllHandler::new(temp_dir.clone())
            .with_max_age(Duration::from_millis(10));

        // Create a file and mark it active
        let active_file = temp_dir.join("active.dll");
        fs::write(&active_file, b"active").unwrap();
        handler.active_copies.insert(active_file.clone(), PathBuf::from("original.dll"));

        // Wait for it to age
        thread::sleep(Duration::from_millis(20));

        // Clean up should skip it
        let cleaned = handler.cleanup_old_copies().unwrap();

        assert_eq!(cleaned, 0);
        assert!(active_file.exists());
    }

    #[test]
    fn test_cleanup_all() {
        let temp = tempdir().unwrap();
        let temp_dir = temp.path().join("temp");
        fs::create_dir_all(&temp_dir).unwrap();

        let mut handler = WindowsDllHandler::new(temp_dir.clone());

        // Create files
        let file1 = temp_dir.join("test1.dll");
        let file2 = temp_dir.join("test2.so");
        let active = temp_dir.join("active.dylib");

        fs::write(&file1, b"1").unwrap();
        fs::write(&file2, b"2").unwrap();
        fs::write(&active, b"active").unwrap();

        handler.active_copies.insert(active.clone(), PathBuf::from("original.dylib"));

        // Clean up all
        let cleaned = handler.cleanup_all().unwrap();

        assert_eq!(cleaned, 2);
        assert!(!file1.exists());
        assert!(!file2.exists());
        assert!(active.exists()); // Active file preserved
    }
}
