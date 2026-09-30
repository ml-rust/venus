//! Universe builder for Venus notebooks.
//!
//! The "Universe" is a shared library containing all external dependencies
//! that cells can link against. It's compiled once with LLVM and cached.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::graph::DefinitionCell;

use super::super::definition_processor::process_definitions;
use super::super::dependency_parser::{DependencyParser, ExternalDependency};
use super::super::notebook_manifest::{NotebookContext, ResolvedManifest};
use super::super::toolchain::ToolchainManager;
use super::super::types::{CompilerConfig, dylib_extension, dylib_prefix};

/// Builder for the Universe shared library.
pub struct UniverseBuilder {
    /// Compiler configuration
    pub(super) config: CompilerConfig,

    /// Toolchain manager (reserved for future LLVM compilation options).
    ///
    /// Currently unused but intentionally preserved to avoid breaking API changes
    /// when LLVM backend support is added. The universe is currently built using
    /// Cargo/rustc, but future versions may support direct LLVM compilation for
    /// optimization control and faster compilation times.
    ///
    /// Keeping this field now prevents:
    /// - Breaking API changes to `UniverseBuilder::new()`
    /// - Re-threading toolchain manager through the codebase later
    /// - Inconsistency with cell compilation (which does use toolchain)
    #[allow(dead_code)]
    toolchain: ToolchainManager,

    /// Dependency parser (handles parsing and hashing)
    parser: DependencyParser,

    /// User-defined type definitions extracted from notebook (promoted to `pub`,
    /// with rkyv derives applied).
    pub(super) type_definitions: String,

    /// Notebook `use` statements from definition cells, re-exported as `pub use`
    /// so imported names are visible to every compiled cell.
    pub(super) imports: String,

    context: NotebookContext,
    pub(super) resolved: ResolvedManifest,
    cache_salt: Option<uuid::Uuid>,
    resolution_error: Option<String>,
}

impl UniverseBuilder {
    /// Create a new universe builder.
    pub fn new(
        config: CompilerConfig,
        toolchain: ToolchainManager,
        workspace_cargo_toml: Option<PathBuf>,
    ) -> Self {
        let mut builder = Self {
            config,
            toolchain,
            parser: DependencyParser::new(),
            type_definitions: String::new(),
            imports: String::new(),
            context: NotebookContext::from_manifest(workspace_cargo_toml),
            resolved: ResolvedManifest::default(),
            cache_salt: None,
            resolution_error: None,
        };
        if let Err(error) = builder.parse_dependencies("", &[]) {
            builder.resolution_error = Some(error.to_string());
        }
        builder
    }

    /// Create a builder with the notebook's Cargo context.
    pub fn for_notebook(
        config: CompilerConfig,
        toolchain: ToolchainManager,
        notebook_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let context = NotebookContext::for_notebook(notebook_path.as_ref())?;
        let mut builder = Self::new(config, toolchain, None);
        builder.context = context;
        if let Err(error) = builder.parse_dependencies("", &[]) {
            builder.resolution_error = Some(error.to_string());
        }
        Ok(builder)
    }

    /// Resolve notebook dependencies and process definition cells.
    ///
    /// Definition cells provide public imports and types with rkyv derives.
    pub fn parse_dependencies(
        &mut self,
        source: &str,
        definition_cells: &[DefinitionCell],
    ) -> Result<()> {
        let resolved = ResolvedManifest::resolve(&self.context, source)?;
        self.parser
            .set_dependencies(resolved.external_dependencies());
        self.resolved = resolved.with_runtime(&self.config, &self.context.directory)?;
        self.resolution_error = None;
        self.cache_salt = self.resolved.has_local_paths().then(uuid::Uuid::new_v4);

        let contents: Vec<String> = definition_cells
            .iter()
            .map(|cell| cell.content.clone())
            .collect();
        let processed = process_definitions(&contents);
        self.imports = processed.imports;
        self.type_definitions = processed.type_definitions;

        Ok(())
    }

    /// Get the dependencies hash (includes imports and type definitions).
    pub fn deps_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.resolved.canonical().hash(&mut hasher);
        self.resolution_error.hash(&mut hasher);
        self.cache_salt.hash(&mut hasher);
        self.config.venus_crate_path.hash(&mut hasher);
        self.config.use_cranelift.hash(&mut hasher);
        self.config.debug_info.hash(&mut hasher);
        self.config.opt_level.hash(&mut hasher);
        self.config.extra_rustc_flags.hash(&mut hasher);
        self.imports.hash(&mut hasher);
        self.type_definitions.hash(&mut hasher);
        hasher.finish()
    }

    /// Get the parsed external dependencies.
    pub fn dependencies(&self) -> &[ExternalDependency] {
        self.parser.dependencies()
    }

    /// Check if the cached universe is valid.
    pub fn is_cache_valid(&self) -> bool {
        if self.resolution_error.is_some() || self.resolved.has_local_paths() {
            return false;
        }
        let cache_file = self.cache_hash_file();
        if !cache_file.exists() {
            return false;
        }

        // Read cached hash
        if let Ok(cached_hash) = fs::read_to_string(&cache_file)
            && let Ok(hash) = cached_hash.trim().parse::<u64>()
        {
            return hash == self.deps_hash();
        }

        false
    }

    /// Get the path to the compiled universe library.
    pub fn universe_path(&self) -> PathBuf {
        let build_dir = self.config.universe_build_dir();
        let filename = format!("{}venus_universe.{}", dylib_prefix(), dylib_extension());
        build_dir.join(filename)
    }

    /// Build the universe library.
    pub fn build(&self) -> Result<PathBuf> {
        if let Some(message) = &self.resolution_error {
            return Err(Error::Compilation {
                cell_id: None,
                message: message.clone(),
            });
        }
        // Check cache first
        if self.is_cache_valid() && self.universe_path().exists() {
            tracing::info!("Using cached universe library");
            return Ok(self.universe_path());
        }

        tracing::info!(
            "Building universe library with {} dependencies",
            self.dependencies().len()
        );

        let build_dir = self.config.universe_build_dir();
        fs::create_dir_all(&build_dir)?;

        // Generate Cargo.toml
        let cargo_toml = self.generate_cargo_toml()?;
        let cargo_path = build_dir.join("Cargo.toml");
        fs::write(&cargo_path, cargo_toml)?;
        fs::write(build_dir.join("build.rs"), self.resolved.build_script())?;

        // Generate lib.rs
        let lib_rs = self.generate_lib_rs();
        let src_dir = build_dir.join("src");
        fs::create_dir_all(&src_dir)?;
        fs::write(src_dir.join("lib.rs"), lib_rs)?;

        // Generate stub notebook.rs (required by lib.rs `pub mod notebook;`)
        // The server overwrites this with real cell content for LSP analysis.
        // For CLI builds, this stub satisfies the module declaration.
        let notebook_rs = "//! Notebook cells module.\n\
                          //! This file is populated by the server for LSP analysis.\n\
                          //! For CLI builds, this is a stub to satisfy the module declaration.\n";
        fs::write(src_dir.join("notebook.rs"), notebook_rs)?;

        // Build with cargo
        let output = Command::new("cargo")
            .current_dir(&build_dir)
            .args(["build", "--release", "--lib"])
            .output()
            .map_err(|e| Error::Compilation {
                cell_id: None,
                message: format!("Failed to run cargo: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Compilation {
                cell_id: None,
                message: format!("Universe build failed:\n{}", stderr),
            });
        }

        // Copy the built library
        let target_lib = build_dir.join("target").join("release").join(format!(
            "{}venus_universe.{}",
            dylib_prefix(),
            dylib_extension()
        ));

        let dest = self.universe_path();
        fs::copy(&target_lib, &dest)?;

        // Save cache hash
        self.save_cache_hash()?;

        tracing::info!("Universe library built: {}", dest.display());
        Ok(dest)
    }

    /// Get the cache hash file path.
    fn cache_hash_file(&self) -> PathBuf {
        self.config.cache_dir.join("universe_hash")
    }

    /// Save the current hash to cache.
    fn save_cache_hash(&self) -> Result<()> {
        let cache_dir = &self.config.cache_dir;
        fs::create_dir_all(cache_dir)?;

        let hash_file = self.cache_hash_file();
        fs::write(&hash_file, self.deps_hash().to_string())?;

        Ok(())
    }
}
