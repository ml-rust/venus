//! Universe builder for Venus notebooks.
//!
//! The "Universe" is a shared library containing all external dependencies
//! that cells can link against. It's compiled once with LLVM and cached.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::error::{Error, Result};
use crate::graph::DefinitionCell;

use super::definition_processor::process_definitions;
use super::dependency_parser::{DependencyParser, ExternalDependency};
use super::toolchain::ToolchainManager;
use super::types::{CompilerConfig, dylib_extension, dylib_prefix};

/// Builder for the Universe shared library.
pub struct UniverseBuilder {
    /// Compiler configuration
    config: CompilerConfig,

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
    type_definitions: String,

    /// Notebook `use` statements from definition cells, re-exported as `pub use`
    /// so imported names are visible to every compiled cell.
    imports: String,

    /// Path to workspace Cargo.toml (for copying dependencies)
    workspace_cargo_toml: Option<PathBuf>,
}

impl UniverseBuilder {
    /// Create a new universe builder.
    pub fn new(
        config: CompilerConfig,
        toolchain: ToolchainManager,
        workspace_cargo_toml: Option<PathBuf>,
    ) -> Self {
        Self {
            config,
            toolchain,
            parser: DependencyParser::new(),
            type_definitions: String::new(),
            imports: String::new(),
            workspace_cargo_toml,
        }
    }

    /// Parse dependencies and process definition cells from notebook source.
    ///
    /// Delegates to [`DependencyParser`] for dependency parsing and to
    /// [`process_definitions`](super::definition_processor::process_definitions)
    /// for turning definition cells into re-exportable imports (`pub use`) and
    /// public type definitions (with rkyv derives applied).
    pub fn parse_dependencies(
        &mut self,
        source: &str,
        definition_cells: &[DefinitionCell],
    ) -> Result<()> {
        self.parser.parse(source);

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
        self.parser.calculate_hash().hash(&mut hasher);
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
        let cargo_toml = self.generate_cargo_toml();
        let cargo_path = build_dir.join("Cargo.toml");
        fs::write(&cargo_path, cargo_toml)?;

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

    /// Copy dependencies from workspace Cargo.toml (if it exists).
    /// Returns the dependencies section as a string.
    fn copy_parent_dependencies(&self) -> String {
        if let Some(cargo_toml_path) = &self.workspace_cargo_toml
            && let Ok(content) = fs::read_to_string(cargo_toml_path)
        {
            // Simple parser: extract [workspace.dependencies] or [dependencies] section
            if let Some(deps_start) = content.find("[workspace.dependencies]") {
                let after_deps = &content[deps_start + "[workspace.dependencies]".len()..];

                // Find next section (starts with '[')
                let deps_end = after_deps.find("\n[").unwrap_or(after_deps.len());
                let deps_section = &after_deps[..deps_end];

                tracing::info!(
                    "Copying workspace dependencies from: {}",
                    cargo_toml_path.display()
                );
                return deps_section.trim().to_string();
            } else if let Some(deps_start) = content.find("[dependencies]") {
                let after_deps = &content[deps_start + "[dependencies]".len()..];

                // Find next section (starts with '[')
                let deps_end = after_deps.find("\n[").unwrap_or(after_deps.len());
                let deps_section = &after_deps[..deps_end];

                tracing::info!("Copying dependencies from: {}", cargo_toml_path.display());
                return deps_section.trim().to_string();
            }
        }

        String::new()
    }

    /// Generate Cargo.toml for the universe crate.
    fn generate_cargo_toml(&self) -> String {
        let mut toml = String::new();

        toml.push_str("[package]\n");
        toml.push_str("name = \"venus_universe\"\n");
        toml.push_str("version = \"0.1.0\"\n");
        toml.push_str("edition = \"2021\"\n");
        toml.push('\n');
        toml.push_str("[lib]\n");
        // cdylib for runtime loading, rlib for cell compilation
        toml.push_str("crate-type = [\"cdylib\", \"rlib\"]\n");
        toml.push('\n');
        toml.push_str("[dependencies]\n");

        // Always include rkyv for zero-copy cell serialization
        toml.push_str("rkyv = { version = \"0.8\", features = [\"std\", \"bytecheck\"] }\n");

        // Always include serde_json for widget JSON parsing in cells
        toml.push_str("serde_json = \"1.0\"\n");

        // Always include venus for widget support
        if let Some(venus_path) = &self.config.venus_crate_path {
            // Use forward slashes for TOML compatibility on Windows
            let path_str = venus_path.display().to_string().replace('\\', "/");
            toml.push_str(&format!("venus = {{ path = \"{path_str}\" }}\n"));
        } else {
            // Use crates.io version when not in development
            toml.push_str("venus = \"0.1\"\n");
        }

        for dep in self.dependencies() {
            // Skip 'venus' dependency if it's already been auto-added above
            // This prevents duplicate key errors in the generated Cargo.toml
            if dep.name == "venus" {
                continue;
            }

            if let Some(path) = &dep.path {
                // Use forward slashes for TOML compatibility on Windows
                let path_str = path.display().to_string().replace('\\', "/");
                toml.push_str(&format!("{} = {{ path = \"{path_str}\" }}\n", dep.name,));
            } else if let Some(version) = &dep.version {
                if dep.features.is_empty() {
                    toml.push_str(&format!("{} = \"{}\"\n", dep.name, version));
                } else {
                    toml.push_str(&format!(
                        "{} = {{ version = \"{}\", features = [{}] }}\n",
                        dep.name,
                        version,
                        dep.features
                            .iter()
                            .map(|f| format!("\"{}\"", f))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }

        // Copy dependencies from parent Cargo.toml (for user's project dependencies)
        // Filter out already-added dependencies to avoid duplicates
        let parent_deps = self.copy_parent_dependencies();
        if !parent_deps.is_empty() {
            toml.push('\n');
            toml.push_str("# Dependencies from parent Cargo.toml\n");

            // Parse and filter out duplicates (venus, rkyv, serde_json)
            for line in parent_deps.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }

                // Extract dependency name (before '=')
                if let Some(dep_name) = trimmed.split('=').next() {
                    let dep_name = dep_name.trim();
                    // Skip if already added or internal venus crates
                    if dep_name == "venus"
                        || dep_name == "venus-macros"
                        || dep_name == "venus-core"
                        || dep_name == "venus-sync"
                        || dep_name == "venus-server"
                        || dep_name == "rkyv"
                        || dep_name == "serde_json"
                        || dep_name == "serde"
                    {
                        continue;
                    }
                }

                toml.push_str(line);
                toml.push('\n');
            }
        }

        // Add empty [workspace] to make this a standalone workspace
        // This prevents it from being pulled into parent workspaces
        toml.push_str("\n[workspace]\n");

        toml
    }

    /// Generate lib.rs that re-exports all dependencies and includes user types.
    fn generate_lib_rs(&self) -> String {
        let mut lib = String::new();

        lib.push_str("//! Venus universe - re-exports all notebook dependencies.\n\n");

        // Allow common lints in generated code
        lib.push_str("#![allow(unused_imports)]\n");
        lib.push_str("#![allow(dead_code)]\n\n");

        // Always re-export rkyv for zero-copy cell serialization
        lib.push_str("pub use rkyv;\n");
        // Re-export derive macros for convenience
        lib.push_str("pub use rkyv::{Archive, Serialize as RkyvSerialize, Deserialize as RkyvDeserialize};\n");
        lib.push_str("pub use rkyv::rancor::Error as RkyvError;\n\n");

        // Re-export serde_json for widget JSON parsing in cell wrappers
        lib.push_str("pub use serde_json;\n\n");

        // Re-export venus widget functions and types for interactive notebooks
        lib.push_str(
            "pub use venus::{input_slider, input_slider_with_step, input_slider_labeled};\n",
        );
        lib.push_str("pub use venus::{input_text, input_text_with_default, input_text_labeled};\n");
        lib.push_str("pub use venus::{input_select, input_select_labeled};\n");
        lib.push_str("pub use venus::{input_checkbox, input_checkbox_labeled};\n");
        lib.push_str("pub use venus::widgets::{WidgetContext, WidgetValue, WidgetDef};\n");
        lib.push_str("pub use venus::widgets::{set_widget_context, take_widget_context};\n\n");

        for dep in self.dependencies() {
            // Convert crate name to valid Rust identifier
            let ident = dep.name.replace('-', "_");
            // Skip rkyv and venus since we already exported them above
            if ident != "rkyv" && ident != "venus" {
                lib.push_str(&format!("pub use {};\n", ident));
            }
        }

        // Re-export notebook imports so their names are visible to cells (which
        // link this crate and glob-import it via `use venus_universe::*;`).
        if !self.imports.is_empty() {
            lib.push_str("\n// Notebook imports (re-exported for cells)\n");
            lib.push_str(&self.imports);
            lib.push('\n');
        }

        // Include user-defined type definitions from the notebook
        if !self.type_definitions.is_empty() {
            lib.push_str("\n// User-defined types from notebook\n");
            lib.push_str(&self.type_definitions);
        }

        // Include notebook module for LSP analysis
        // This module is written by the server with current cell content
        lib.push_str("\n// Notebook cells (for LSP analysis)\n");
        lib.push_str("pub mod notebook;\n");

        lib
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CellId, DefinitionType, SourceSpan};

    fn make_builder() -> UniverseBuilder {
        let config = CompilerConfig::default();
        let toolchain = ToolchainManager::new().unwrap();
        UniverseBuilder::new(config, toolchain, None)
    }

    /// Build a definition cell of the given type with the given content.
    fn def_cell(content: &str, definition_type: DefinitionType) -> DefinitionCell {
        DefinitionCell {
            id: CellId::new(0),
            content: content.to_string(),
            definition_type,
            span: SourceSpan {
                start_line: 1,
                start_col: 0,
                end_line: 1,
                end_col: content.len(),
            },
            source_file: PathBuf::from("notebook.rs"),
            doc_comment: None,
        }
    }

    /// Assert that a notebook `use` statement referencing `path_fragment` appears
    /// in the generated universe and is re-exported (`pub use`), never left as a
    /// private `use`. A private `use` in the universe crate is invisible to cells
    /// (which link the universe and glob-import it), which is exactly the failure
    /// where a notebook import "doesn't work" for a cell that names the type.
    fn assert_import_reexported(lib: &str, path_fragment: &str) {
        // The universe is rendered from `syn` tokens, which space out `::`, so
        // compare with all whitespace removed.
        let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let needle = strip(path_fragment);

        let matching: Vec<&str> = lib
            .lines()
            .map(|l| l.trim())
            .filter(|l| l.contains("use") && strip(l).contains(&needle))
            .collect();

        assert!(
            !matching.is_empty(),
            "expected an import referencing `{path_fragment}` in generated universe, found none:\n{lib}"
        );

        for line in matching {
            assert!(
                strip(line).starts_with("pubuse"),
                "notebook import `{line}` must be re-exported as `pub use` so cells can see it, \
                 but it was emitted as a private `use` (invisible through `use venus_universe::*`)"
            );
        }
    }

    #[test]
    fn universe_reexports_named_import_from_definition_cell() {
        let mut builder = make_builder();
        let cells = vec![def_cell(
            "use plotlars::polars::prelude::DataFrame;",
            DefinitionType::Import,
        )];
        builder.parse_dependencies("", &cells).unwrap();

        let lib = builder.generate_lib_rs();
        assert_import_reexported(&lib, "plotlars::polars::prelude::DataFrame");
    }

    #[test]
    fn universe_reexports_glob_import_from_definition_cell() {
        let mut builder = make_builder();
        let cells = vec![def_cell("use venus::prelude::*;", DefinitionType::Import)];
        builder.parse_dependencies("", &cells).unwrap();

        let lib = builder.generate_lib_rs();
        assert_import_reexported(&lib, "venus::prelude::*");
    }

    #[test]
    fn universe_reexports_aliased_import_from_definition_cell() {
        let mut builder = make_builder();
        let cells = vec![def_cell(
            "use std::collections::HashMap as Map;",
            DefinitionType::Import,
        )];
        builder.parse_dependencies("", &cells).unwrap();

        let lib = builder.generate_lib_rs();
        assert_import_reexported(&lib, "std::collections::HashMap as Map");
    }

    #[test]
    fn universe_reexports_multiple_imports_in_one_cell() {
        let mut builder = make_builder();
        let cells = vec![def_cell(
            "use std::collections::HashMap;\nuse std::collections::BTreeMap;",
            DefinitionType::Import,
        )];
        builder.parse_dependencies("", &cells).unwrap();

        let lib = builder.generate_lib_rs();
        assert_import_reexported(&lib, "std::collections::HashMap");
        assert_import_reexported(&lib, "std::collections::BTreeMap");
    }

    #[test]
    fn universe_reexports_use_from_mixed_definition_cell() {
        let mut builder = make_builder();
        // A single definition cell that mixes a `use` with a type definition is
        // classified as `Mixed`; its import must still reach cells, and its type
        // must still be present.
        let cells = vec![def_cell(
            "use std::collections::HashMap;\n\npub struct Config {\n    pub map: HashMap<String, i32>,\n}",
            DefinitionType::Mixed,
        )];
        builder.parse_dependencies("", &cells).unwrap();

        let lib = builder.generate_lib_rs();
        assert_import_reexported(&lib, "std::collections::HashMap");
        assert!(
            lib.contains("struct Config"),
            "type definition from a mixed cell must still be present in the universe:\n{lib}"
        );
    }

    #[test]
    fn test_parse_simple_dependency() {
        let mut builder = make_builder();

        let source = r#"
//! # My Notebook
//!
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```

#[venus::cell]
pub fn hello() -> i32 { 42 }
"#;

        builder.parse_dependencies(source, &[]).unwrap();

        assert_eq!(builder.dependencies().len(), 1);
        assert_eq!(builder.dependencies()[0].name, "serde");
        assert_eq!(builder.dependencies()[0].version, Some("1.0".to_string()));
    }

    #[test]
    fn test_parse_complex_dependency() {
        let mut builder = make_builder();

        let source = r#"
//! ```cargo
//! [dependencies]
//! tokio = { version = "1", features = ["full"] }
//! ```
"#;

        builder.parse_dependencies(source, &[]).unwrap();

        assert_eq!(builder.dependencies().len(), 1);
        assert_eq!(builder.dependencies()[0].name, "tokio");
        assert_eq!(builder.dependencies()[0].version, Some("1".to_string()));
        assert_eq!(builder.dependencies()[0].features, vec!["full"]);
    }

    #[test]
    fn test_parse_multiple_dependencies() {
        let mut builder = make_builder();

        let source = r#"
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! serde_json = "1.0"
//! tokio = { version = "1", features = ["rt", "macros"] }
//! ```
"#;

        builder.parse_dependencies(source, &[]).unwrap();

        assert_eq!(builder.dependencies().len(), 3);
    }

    #[test]
    fn test_generate_cargo_toml() {
        let mut builder = make_builder();

        // Parse a dependency with features
        let source = r#"
//! ```cargo
//! [dependencies]
//! serde = { version = "1.0", features = ["derive"] }
//! ```
"#;
        builder.parse_dependencies(source, &[]).unwrap();

        let toml = builder.generate_cargo_toml();
        assert!(toml.contains("[package]"));
        assert!(toml.contains("venus_universe"));
        assert!(toml.contains("serde"));
        assert!(toml.contains("derive"));
    }

    #[test]
    fn test_hash_changes_with_deps() {
        let mut builder = make_builder();

        builder.parse_dependencies("", &[]).unwrap();
        let hash1 = builder.deps_hash();

        builder
            .parse_dependencies(
                r#"
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```
"#,
                &[],
            )
            .unwrap();
        let hash2 = builder.deps_hash();

        assert_ne!(hash1, hash2);
    }
}
