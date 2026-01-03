//! Universe builder for Venus notebooks.
//!
//! The "Universe" is a shared library containing all external dependencies
//! that cells can link against. It's compiled once with LLVM and cached.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::error::{Error, Result};

use super::dependency_parser::{DependencyParser, ExternalDependency};
use super::toolchain::ToolchainManager;
use super::types::{CompilerConfig, dylib_extension, dylib_prefix};

/// Builder for the Universe shared library.
pub struct UniverseBuilder {
    /// Compiler configuration
    config: CompilerConfig,

    /// Toolchain manager (reserved for future LLVM compilation options)
    #[allow(dead_code)]
    toolchain: ToolchainManager,

    /// Dependency parser (handles parsing and hashing)
    parser: DependencyParser,

    /// User-defined type definitions extracted from notebook
    type_definitions: String,
}

impl UniverseBuilder {
    /// Create a new universe builder.
    pub fn new(config: CompilerConfig, toolchain: ToolchainManager) -> Self {
        Self {
            config,
            toolchain,
            parser: DependencyParser::new(),
            type_definitions: String::new(),
        }
    }

    /// Parse dependencies and extract type definitions from notebook source.
    ///
    /// Delegates to [`DependencyParser`] for dependency parsing and extracts
    /// user-defined types (structs, enums, type aliases) for inclusion in the universe.
    pub fn parse_dependencies(&mut self, source: &str) -> Result<()> {
        self.parser.parse(source);
        self.type_definitions = Self::extract_type_definitions(source);
        Ok(())
    }

    /// Extract type definitions (structs, enums, type aliases) from notebook source.
    ///
    /// This finds all type definitions that are NOT cells (not marked with #[venus::cell])
    /// so they can be included in the universe for cells to use.
    fn extract_type_definitions(source: &str) -> String {
        let mut types = String::new();
        let mut in_type_def = false;
        let mut brace_depth: i32 = 0;
        let mut current_def = String::new();
        let mut skip_next_item = false;

        for line in source.lines() {
            let trimmed = line.trim();

            // Skip venus::cell functions - they're handled separately
            if trimmed.starts_with("#[venus::cell]") || trimmed.starts_with("#[venus :: cell]") {
                skip_next_item = true;
                continue;
            }

            // Skip module-level doc comments and cargo blocks
            if trimmed.starts_with("//!") {
                continue;
            }

            // Check if this is the start of a type definition
            let is_type_start = trimmed.starts_with("pub struct ")
                || trimmed.starts_with("struct ")
                || trimmed.starts_with("pub enum ")
                || trimmed.starts_with("enum ")
                || trimmed.starts_with("pub type ")
                || trimmed.starts_with("type ")
                || trimmed.starts_with("#[derive");

            // Skip function definitions that aren't cells
            let is_fn_start = trimmed.starts_with("pub fn ")
                || trimmed.starts_with("fn ")
                || trimmed.starts_with("pub async fn ")
                || trimmed.starts_with("async fn ");

            if skip_next_item && is_fn_start {
                // Skip this function (it's a cell)
                skip_next_item = false;
                // Track braces to skip the entire function body
                brace_depth = 0;
                for ch in line.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth = brace_depth.saturating_sub(1),
                        _ => {}
                    }
                }
                if brace_depth > 0 {
                    in_type_def = true; // Reuse flag to track skipping
                    current_def.clear();
                }
                continue;
            }

            // If we're skipping a cell function body
            if in_type_def && current_def.is_empty() {
                for ch in line.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth = brace_depth.saturating_sub(1),
                        _ => {}
                    }
                }
                if brace_depth == 0 {
                    in_type_def = false;
                }
                continue;
            }

            // Handle derive attributes - include them with the following type
            if trimmed.starts_with("#[derive") || trimmed.starts_with("#[") {
                if !in_type_def {
                    current_def.push_str(line);
                    current_def.push('\n');
                }
                continue;
            }

            if is_type_start && !in_type_def {
                in_type_def = true;
                current_def.push_str(line);
                current_def.push('\n');

                // Count braces to track when type ends
                for ch in line.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth = brace_depth.saturating_sub(1),
                        _ => {}
                    }
                }

                // Single-line type alias
                if trimmed.contains("type ") && trimmed.ends_with(';') {
                    types.push_str(&current_def);
                    types.push('\n');
                    current_def.clear();
                    in_type_def = false;
                }
                // Single-line struct/enum
                else if brace_depth == 0 && trimmed.ends_with(';') {
                    types.push_str(&current_def);
                    types.push('\n');
                    current_def.clear();
                    in_type_def = false;
                }
            } else if in_type_def {
                current_def.push_str(line);
                current_def.push('\n');

                for ch in line.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth = brace_depth.saturating_sub(1),
                        _ => {}
                    }
                }

                if brace_depth == 0 {
                    types.push_str(&current_def);
                    types.push('\n');
                    current_def.clear();
                    in_type_def = false;
                }
            }

            skip_next_item = false;
        }

        // Transform serde derives to rkyv derives
        Self::transform_derives_to_rkyv(&types)
    }

    /// Transform derives to include rkyv serialization for all user types.
    ///
    /// All user-defined types (structs/enums) need rkyv derives since cells
    /// serialize their return values. This function adds Archive, RkyvSerialize,
    /// and RkyvDeserialize while preserving existing derives like Debug, Clone.
    fn transform_derives_to_rkyv(source: &str) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let mut result = String::new();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim();

            if trimmed.starts_with("#[derive(") {
                // Look ahead to see if this is for a struct/enum
                let mut is_type_def = false;
                for j in (i + 1)..lines.len() {
                    let next = lines[j].trim();
                    if next.is_empty() || next.starts_with("//") || next.starts_with("#[") {
                        continue; // Skip comments and other attributes
                    }
                    if next.starts_with("pub struct ") || next.starts_with("struct ")
                        || next.starts_with("pub enum ") || next.starts_with("enum ") {
                        is_type_def = true;
                    }
                    break; // Found the item this derive is for
                }

                if is_type_def {
                    // Extract and transform derives
                    if let Some(start) = trimmed.find('(') {
                        if let Some(end) = trimmed.rfind(')') {
                            let derives = &trimmed[start + 1..end];
                            let mut new_derives: Vec<&str> = Vec::new();
                            let mut has_rkyv = false;

                            for derive in derives.split(',').map(|s| s.trim()) {
                                match derive {
                                    "Serialize" | "Deserialize" => {
                                        // Skip serde derives, we'll add rkyv
                                    }
                                    "Archive" | "RkyvSerialize" | "RkyvDeserialize" => {
                                        // Already has rkyv derives
                                        has_rkyv = true;
                                        new_derives.push(derive);
                                    }
                                    other if !other.is_empty() => {
                                        new_derives.push(other);
                                    }
                                    _ => {}
                                }
                            }

                            // Always add rkyv derives for structs/enums if not already present
                            if !has_rkyv {
                                new_derives.push("Archive");
                                new_derives.push("RkyvSerialize");
                                new_derives.push("RkyvDeserialize");
                            }

                            // Reconstruct the derive line
                            result.push_str(&format!("#[derive({})]\n", new_derives.join(", ")));

                            // Add rkyv attribute for archived type derives
                            result.push_str("#[rkyv(derive(Debug))]\n");
                            i += 1;
                            continue;
                        }
                    }
                }
            }

            result.push_str(line);
            result.push('\n');
            i += 1;
        }

        result
    }

    /// Get the dependencies hash (includes type definitions).
    pub fn deps_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.parser.calculate_hash().hash(&mut hasher);
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
            toml.push_str(&format!(
                "venus = {{ path = \"{}\" }}\n",
                venus_path.display()
            ));
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
                toml.push_str(&format!(
                    "{} = {{ path = \"{}\" }}\n",
                    dep.name,
                    path.display()
                ));
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
        lib.push_str("pub use venus::{input_slider, input_slider_with_step, input_slider_labeled};\n");
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

        // Include user-defined type definitions from the notebook
        if !self.type_definitions.is_empty() {
            lib.push_str("\n// User-defined types from notebook\n");
            lib.push_str(&self.type_definitions);
        }

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

    fn make_builder() -> UniverseBuilder {
        let config = CompilerConfig::default();
        let toolchain = ToolchainManager::new().unwrap();
        UniverseBuilder::new(config, toolchain)
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

        builder.parse_dependencies(source).unwrap();

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

        builder.parse_dependencies(source).unwrap();

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

        builder.parse_dependencies(source).unwrap();

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
        builder.parse_dependencies(source).unwrap();

        let toml = builder.generate_cargo_toml();
        assert!(toml.contains("[package]"));
        assert!(toml.contains("venus_universe"));
        assert!(toml.contains("serde"));
        assert!(toml.contains("derive"));
    }

    #[test]
    fn test_hash_changes_with_deps() {
        let mut builder = make_builder();

        builder.parse_dependencies("").unwrap();
        let hash1 = builder.deps_hash();

        builder
            .parse_dependencies(
                r#"
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```
"#,
            )
            .unwrap();
        let hash2 = builder.deps_hash();

        assert_ne!(hash1, hash2);
    }
}
