//! Common types for the compilation pipeline.

use std::path::PathBuf;

use crate::graph::CellId;
use crate::paths::NotebookDirs;

/// Configuration for the compiler.
#[derive(Debug, Clone)]
pub struct CompilerConfig {
    /// Directory for build artifacts (.venus/build/)
    pub build_dir: PathBuf,

    /// Directory for cached outputs (.venus/cache/)
    pub cache_dir: PathBuf,

    /// Use Cranelift backend (fast compilation)
    pub use_cranelift: bool,

    /// Emit debug info
    pub debug_info: bool,

    /// Optimization level (0-3)
    pub opt_level: u8,

    /// Additional rustc flags
    pub extra_rustc_flags: Vec<String>,

    /// Path to the venus crate (for universe compilation).
    /// If None, uses crates.io published version.
    pub venus_crate_path: Option<PathBuf>,
}

impl Default for CompilerConfig {
    fn default() -> Self {
        Self {
            build_dir: PathBuf::from(".venus/build"),
            cache_dir: PathBuf::from(".venus/cache"),
            use_cranelift: true,
            debug_info: true,
            opt_level: 0,
            extra_rustc_flags: Vec::new(),
            venus_crate_path: Self::detect_venus_crate_path(),
        }
    }
}

impl CompilerConfig {
    /// Detect the path to the venus crate.
    ///
    /// During development (running from workspace), returns the path to crates/venus.
    /// In production (installed binary), returns None to use crates.io version.
    fn detect_venus_crate_path() -> Option<PathBuf> {
        // Try to find the venus crate relative to the current executable
        if let Ok(exe_path) = std::env::current_exe() {
            // Check if we're in the target directory of the venus workspace
            // e.g., /path/to/venus/target/release/venus-cli
            if let Some(target_dir) = exe_path.parent() {
                // Go up from target/release or target/debug
                let workspace_root = target_dir.parent().and_then(|p| p.parent());
                if let Some(root) = workspace_root {
                    let venus_crate = root.join("crates").join("venus");
                    if venus_crate.join("Cargo.toml").exists() {
                        return Some(venus_crate);
                    }
                }
            }
        }

        // Fallback: Check CARGO_MANIFEST_DIR (available during cargo test)
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let manifest_path = PathBuf::from(&manifest_dir);
            // Navigate up to workspace root and find crates/venus
            if let Some(workspace_root) = manifest_path
                .ancestors()
                .find(|p| p.join("crates").join("venus").join("Cargo.toml").exists())
            {
                return Some(workspace_root.join("crates").join("venus"));
            }
        }

        // Not in development environment - will use crates.io version
        None
    }

    /// Create config for fast development builds.
    pub fn development() -> Self {
        Self::default()
    }

    /// Create config for optimized production builds.
    pub fn production() -> Self {
        Self {
            use_cranelift: false, // Use LLVM for better optimization
            debug_info: false,
            opt_level: 3,
            ..Default::default()
        }
    }

    /// Create a development config with paths from NotebookDirs.
    ///
    /// This is the recommended way to create a config for interactive use.
    pub fn for_notebook(dirs: &NotebookDirs) -> Self {
        Self {
            build_dir: dirs.build_dir.clone(),
            cache_dir: dirs.cache_dir.clone(),
            ..Self::development()
        }
    }

    /// Create a production config with paths from NotebookDirs.
    ///
    /// This is the recommended way to create a config for optimized builds.
    pub fn for_notebook_release(dirs: &NotebookDirs) -> Self {
        Self {
            build_dir: dirs.build_dir.clone(),
            cache_dir: dirs.cache_dir.clone(),
            ..Self::production()
        }
    }

    /// Get the path for cell build artifacts.
    pub fn cell_build_dir(&self) -> PathBuf {
        self.build_dir.join("cells")
    }

    /// Get the path for universe build artifacts.
    pub fn universe_build_dir(&self) -> PathBuf {
        self.build_dir.join("universe")
    }
}

/// Result of compiling a cell.
#[derive(Debug, Clone)]
pub struct CompiledCell {
    /// Cell identifier
    pub cell_id: CellId,

    /// Cell name
    pub name: String,

    /// Path to the compiled dynamic library
    pub dylib_path: PathBuf,

    /// Entry point symbol name
    pub entry_symbol: String,

    /// Hash of the cell source (for cache invalidation)
    pub source_hash: u64,

    /// Hash of dependencies (for cache invalidation)
    pub deps_hash: u64,

    /// Compilation time in milliseconds
    pub compile_time_ms: u64,
}

/// Result of a compilation operation.
#[derive(Debug)]
pub enum CompilationResult {
    /// Compilation succeeded
    Success(CompiledCell),

    /// Compilation failed with errors
    Failed {
        cell_id: CellId,
        errors: Vec<crate::compile::CompileError>,
    },

    /// Used cached result (no recompilation needed)
    Cached(CompiledCell),
}

impl CompilationResult {
    /// Returns true if compilation was successful (or cached).
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_) | Self::Cached(_))
    }

    /// Get the compiled cell if successful.
    pub fn compiled_cell(&self) -> Option<&CompiledCell> {
        match self {
            Self::Success(cell) | Self::Cached(cell) => Some(cell),
            Self::Failed { .. } => None,
        }
    }
}

/// Platform-specific dynamic library extension.
pub fn dylib_extension() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "dll"
    }
    #[cfg(target_os = "macos")]
    {
        "dylib"
    }
    #[cfg(target_os = "linux")]
    {
        "so"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "so" // Default to .so for unknown platforms
    }
}

/// Platform-specific dynamic library prefix.
pub fn dylib_prefix() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        ""
    }
    #[cfg(not(target_os = "windows"))]
    {
        "lib"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = CompilerConfig::default();
        assert!(config.use_cranelift);
        assert!(config.debug_info);
        assert_eq!(config.opt_level, 0);
    }

    #[test]
    fn test_production_config() {
        let config = CompilerConfig::production();
        assert!(!config.use_cranelift);
        assert!(!config.debug_info);
        assert_eq!(config.opt_level, 3);
    }

    #[test]
    fn test_dylib_extension() {
        let ext = dylib_extension();
        #[cfg(target_os = "linux")]
        assert_eq!(ext, "so");
        #[cfg(target_os = "macos")]
        assert_eq!(ext, "dylib");
        #[cfg(target_os = "windows")]
        assert_eq!(ext, "dll");
    }
}
