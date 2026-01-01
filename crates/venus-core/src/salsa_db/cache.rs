//! Cache persistence for Salsa DB using rkyv.
//!
//! This module provides disk-based persistence of compilation state,
//! enabling instant resume on startup by avoiding recompilation of
//! unchanged cells.
//!
//! # Cache Structure
//!
//! The cache stores:
//! - Toolchain version for validation
//! - Dependency hash for universe invalidation
//! - Compilation results per cell (dylib paths, source hashes)
//!
//! # Usage
//!
//! ```ignore
//! use venus_core::salsa_db::{VenusDatabase, CachePersistence};
//!
//! let db = VenusDatabase::new();
//! let cache_path = PathBuf::from(".venus/cache/salsa.bin");
//!
//! // Load existing cache if valid
//! if let Some(snapshot) = CachePersistence::load(&cache_path, "nightly-2024-01-15")? {
//!     db.restore_from_snapshot(&snapshot);
//! }
//!
//! // ... work with db ...
//!
//! // Save cache on exit
//! let snapshot = db.create_snapshot("nightly-2024-01-15", dep_hash);
//! CachePersistence::save(&cache_path, &snapshot)?;
//! ```

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rkyv::{rancor, Archive, Deserialize, Serialize};

/// Current cache format version.
///
/// Increment this when the cache format changes in an incompatible way.
/// Old caches with different versions will be automatically invalidated.
pub const CACHE_VERSION: u32 = 1;

/// Snapshot of Salsa DB state that can be persisted to disk.
///
/// This captures the essential compilation state needed for instant resume:
/// - Which cells have been compiled
/// - Where their dylibs are located
/// - What source code produced them (via hash)
///
/// Note: This does NOT store Salsa's internal memoization state.
/// Salsa queries will be recomputed on load, but since compiled dylibs
/// are preserved, compilation (the slow part) is skipped.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct CacheSnapshot {
    /// Cache format version for compatibility checking.
    pub version: u32,

    /// Rust toolchain version string (e.g., "rustc 1.76.0-nightly (abc123 2024-01-15)").
    ///
    /// Cache is invalidated if toolchain changes, since compiled dylibs
    /// may have ABI incompatibilities.
    pub toolchain_version: String,

    /// Hash of external dependencies from `//! [dependencies]` block.
    ///
    /// If this changes, the universe dylib needs recompilation and
    /// all cells must be recompiled against the new universe.
    pub dependency_hash: u64,

    /// Compilation results keyed by cell name.
    pub cells: HashMap<String, CachedCell>,

    /// Unix timestamp when cache was created.
    pub created_at: u64,
}

/// Cached compilation result for a single cell.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub struct CachedCell {
    /// Name of the cell function.
    pub name: String,

    /// Hash of the cell's source code.
    ///
    /// Used to detect if the cell has changed since compilation.
    pub source_hash: u64,

    /// Path to compiled dylib (relative to cache directory).
    ///
    /// Empty string if compilation failed.
    pub dylib_path: String,

    /// Compilation status.
    pub status: CachedCompilationStatus,
}

/// Cached compilation status.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum CachedCompilationStatus {
    /// Cell compiled successfully.
    Success,

    /// Compilation was skipped (used cached dylib).
    Cached,

    /// Compilation failed with error message.
    Failed { error: String },
}

/// Error type for cache operations.
#[derive(Debug)]
pub enum CacheError {
    /// IO error reading/writing cache file.
    Io(io::Error),

    /// Cache format version mismatch.
    VersionMismatch { expected: u32, found: u32 },

    /// Toolchain version mismatch.
    ToolchainMismatch { expected: String, found: String },

    /// Dependency hash mismatch (universe needs recompilation).
    DependencyMismatch { expected: u64, found: u64 },

    /// Failed to deserialize cache data.
    Deserialize(String),

    /// Failed to serialize cache data.
    Serialize(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::Io(e) => write!(f, "cache IO error: {}", e),
            CacheError::VersionMismatch { expected, found } => {
                write!(
                    f,
                    "cache version mismatch: expected {}, found {}",
                    expected, found
                )
            }
            CacheError::ToolchainMismatch { expected, found } => {
                write!(
                    f,
                    "toolchain mismatch: expected '{}', found '{}'",
                    expected, found
                )
            }
            CacheError::DependencyMismatch { expected, found } => {
                write!(
                    f,
                    "dependency hash mismatch: expected {:#x}, found {:#x}",
                    expected, found
                )
            }
            CacheError::Deserialize(e) => write!(f, "cache deserialize error: {}", e),
            CacheError::Serialize(e) => write!(f, "cache serialize error: {}", e),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CacheError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for CacheError {
    fn from(e: io::Error) -> Self {
        CacheError::Io(e)
    }
}

/// Cache persistence operations.
pub struct CachePersistence;

impl CachePersistence {
    /// Save a cache snapshot to disk.
    ///
    /// Creates parent directories if they don't exist.
    /// Uses atomic write (write to temp file, then rename) to prevent corruption.
    pub fn save(path: &Path, snapshot: &CacheSnapshot) -> Result<(), CacheError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Serialize with rkyv
        let bytes = rkyv::to_bytes::<rancor::Error>(snapshot)
            .map_err(|e| CacheError::Serialize(e.to_string()))?;

        // Write to temp file first for atomic operation
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;

        // Atomic rename
        fs::rename(&temp_path, path)?;

        tracing::debug!(
            "Saved cache snapshot: {} cells, {} bytes",
            snapshot.cells.len(),
            bytes.len()
        );

        Ok(())
    }

    /// Load a cache snapshot from disk.
    ///
    /// Returns `Ok(None)` if the cache file doesn't exist.
    /// Returns `Err` if the cache exists but is invalid or incompatible.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the cache file
    /// * `expected_toolchain` - Current toolchain version; cache is invalidated if different
    pub fn load(path: &Path, expected_toolchain: &str) -> Result<Option<CacheSnapshot>, CacheError> {
        // Check if cache exists
        if !path.exists() {
            tracing::debug!("No cache file at {:?}", path);
            return Ok(None);
        }

        // Read cache file
        let mut file = fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        // Deserialize with validation
        let archived = rkyv::access::<ArchivedCacheSnapshot, rancor::Error>(&bytes)
            .map_err(|e| CacheError::Deserialize(e.to_string()))?;

        // Check cache version
        let found_version: u32 = archived.version.into();
        if found_version != CACHE_VERSION {
            return Err(CacheError::VersionMismatch {
                expected: CACHE_VERSION,
                found: found_version,
            });
        }

        // Deserialize fully
        let snapshot: CacheSnapshot =
            rkyv::deserialize::<CacheSnapshot, rancor::Error>(archived)
                .map_err(|e| CacheError::Deserialize(e.to_string()))?;

        // Check toolchain version
        if snapshot.toolchain_version != expected_toolchain {
            return Err(CacheError::ToolchainMismatch {
                expected: expected_toolchain.to_string(),
                found: snapshot.toolchain_version.clone(),
            });
        }

        tracing::debug!(
            "Loaded cache snapshot: {} cells, created at {}",
            snapshot.cells.len(),
            snapshot.created_at
        );

        Ok(Some(snapshot))
    }

    /// Load a cache snapshot without toolchain validation.
    ///
    /// Use this when you want to inspect the cache or handle
    /// validation separately.
    pub fn load_unchecked(path: &Path) -> Result<Option<CacheSnapshot>, CacheError> {
        if !path.exists() {
            return Ok(None);
        }

        let mut file = fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        let archived = rkyv::access::<ArchivedCacheSnapshot, rancor::Error>(&bytes)
            .map_err(|e| CacheError::Deserialize(e.to_string()))?;

        let found_version: u32 = archived.version.into();
        if found_version != CACHE_VERSION {
            return Err(CacheError::VersionMismatch {
                expected: CACHE_VERSION,
                found: found_version,
            });
        }

        let snapshot: CacheSnapshot =
            rkyv::deserialize::<CacheSnapshot, rancor::Error>(archived)
                .map_err(|e| CacheError::Deserialize(e.to_string()))?;

        Ok(Some(snapshot))
    }

    /// Delete the cache file if it exists.
    pub fn invalidate(path: &Path) -> Result<(), CacheError> {
        if path.exists() {
            fs::remove_file(path)?;
            tracing::debug!("Invalidated cache at {:?}", path);
        }
        Ok(())
    }
}

impl CacheSnapshot {
    /// Create a new cache snapshot.
    pub fn new(toolchain_version: String, dependency_hash: u64) -> Self {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            version: CACHE_VERSION,
            toolchain_version,
            dependency_hash,
            cells: HashMap::new(),
            created_at,
        }
    }

    /// Add a compiled cell to the snapshot.
    pub fn add_cell(&mut self, cell: CachedCell) {
        self.cells.insert(cell.name.clone(), cell);
    }

    /// Get a cached cell by name.
    pub fn get_cell(&self, name: &str) -> Option<&CachedCell> {
        self.cells.get(name)
    }

    /// Check if a cell's source has changed.
    ///
    /// Returns `true` if the cell exists in cache with the same source hash.
    pub fn is_cell_valid(&self, name: &str, current_source_hash: u64) -> bool {
        self.cells
            .get(name)
            .map(|c| c.source_hash == current_source_hash)
            .unwrap_or(false)
    }

    /// Check if dependency hash matches.
    pub fn is_dependency_valid(&self, current_hash: u64) -> bool {
        self.dependency_hash == current_hash
    }
}

impl CachedCell {
    /// Create a new cached cell with successful compilation.
    pub fn success(name: String, source_hash: u64, dylib_path: String) -> Self {
        Self {
            name,
            source_hash,
            dylib_path,
            status: CachedCompilationStatus::Success,
        }
    }

    /// Create a new cached cell using existing cache.
    pub fn cached(name: String, source_hash: u64, dylib_path: String) -> Self {
        Self {
            name,
            source_hash,
            dylib_path,
            status: CachedCompilationStatus::Cached,
        }
    }

    /// Create a new cached cell with failed compilation.
    pub fn failed(name: String, source_hash: u64, error: String) -> Self {
        Self {
            name,
            source_hash,
            dylib_path: String::new(),
            status: CachedCompilationStatus::Failed { error },
        }
    }

    /// Check if the cell compiled successfully (or used cache).
    pub fn is_success(&self) -> bool {
        matches!(
            self.status,
            CachedCompilationStatus::Success | CachedCompilationStatus::Cached
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cache_round_trip() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("test_cache.bin");

        // Create snapshot
        let mut snapshot = CacheSnapshot::new("rustc 1.76.0-nightly".to_string(), 0x12345678);

        snapshot.add_cell(CachedCell::success(
            "cell_a".to_string(),
            0xAABBCCDD,
            "cell_a.so".to_string(),
        ));

        snapshot.add_cell(CachedCell::failed(
            "cell_b".to_string(),
            0x11223344,
            "type mismatch".to_string(),
        ));

        // Save
        CachePersistence::save(&cache_path, &snapshot).unwrap();

        // Load
        let loaded = CachePersistence::load(&cache_path, "rustc 1.76.0-nightly")
            .unwrap()
            .unwrap();

        assert_eq!(loaded.version, CACHE_VERSION);
        assert_eq!(loaded.toolchain_version, "rustc 1.76.0-nightly");
        assert_eq!(loaded.dependency_hash, 0x12345678);
        assert_eq!(loaded.cells.len(), 2);

        let cell_a = loaded.get_cell("cell_a").unwrap();
        assert_eq!(cell_a.source_hash, 0xAABBCCDD);
        assert!(cell_a.is_success());

        let cell_b = loaded.get_cell("cell_b").unwrap();
        assert!(!cell_b.is_success());
        assert!(matches!(
            &cell_b.status,
            CachedCompilationStatus::Failed { error } if error == "type mismatch"
        ));
    }

    #[test]
    fn test_cache_missing_file() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("nonexistent.bin");

        let result = CachePersistence::load(&cache_path, "rustc 1.76.0-nightly").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_toolchain_mismatch() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("test_cache.bin");

        // Save with one toolchain
        let snapshot = CacheSnapshot::new("rustc 1.76.0-nightly".to_string(), 0);
        CachePersistence::save(&cache_path, &snapshot).unwrap();

        // Load with different toolchain
        let result = CachePersistence::load(&cache_path, "rustc 1.77.0-nightly");

        assert!(matches!(
            result,
            Err(CacheError::ToolchainMismatch { .. })
        ));
    }

    #[test]
    fn test_cache_invalidation() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("test_cache.bin");

        // Create cache
        let snapshot = CacheSnapshot::new("test".to_string(), 0);
        CachePersistence::save(&cache_path, &snapshot).unwrap();
        assert!(cache_path.exists());

        // Invalidate
        CachePersistence::invalidate(&cache_path).unwrap();
        assert!(!cache_path.exists());
    }

    #[test]
    fn test_cell_validity() {
        let mut snapshot = CacheSnapshot::new("test".to_string(), 0);

        snapshot.add_cell(CachedCell::success("test".to_string(), 0x1234, "".to_string()));

        // Same hash - valid
        assert!(snapshot.is_cell_valid("test", 0x1234));

        // Different hash - invalid
        assert!(!snapshot.is_cell_valid("test", 0x5678));

        // Unknown cell - invalid
        assert!(!snapshot.is_cell_valid("unknown", 0x1234));
    }

    #[test]
    fn test_dependency_validity() {
        let snapshot = CacheSnapshot::new("test".to_string(), 0xABCD);

        assert!(snapshot.is_dependency_valid(0xABCD));
        assert!(!snapshot.is_dependency_valid(0x1234));
    }
}
