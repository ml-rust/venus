//! External dependency parsing for Venus notebooks.
//!
//! Parses cargo-style dependency specifications from notebook doc comments.
//!
//! # Naming Note
//!
//! This module uses `ExternalDependency` to represent crate dependencies (e.g., `serde = "1.0"`).
//! This is distinct from `graph::Dependency` which represents cell-to-cell parameter dependencies.
//!
//! # Format
//!
//! Dependencies are specified in a `cargo` fenced code block:
//!
//! ```text
//! //! ```cargo
//! //! [dependencies]
//! //! serde = "1.0"
//! //! tokio = { version = "1", features = ["full"] }
//! //! ```
//! ```

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// External crate dependency parsed from a notebook.
///
/// Represents a Cargo dependency specification (e.g., `serde = "1.0"`).
/// Not to be confused with `graph::Dependency` which represents cell parameter dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExternalDependency {
    /// Crate name
    pub name: String,

    /// Version requirement (e.g., "1.0", "^2.0")
    pub version: Option<String>,

    /// Features to enable
    pub features: Vec<String>,

    /// Path dependency (for local crates)
    pub path: Option<PathBuf>,
}

impl ExternalDependency {
    /// Create a simple version dependency.
    pub fn simple(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: Some(version.into()),
            features: Vec::new(),
            path: None,
        }
    }

    /// Create a path dependency.
    pub fn path_dep(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            version: None,
            features: Vec::new(),
            path: Some(path.into()),
        }
    }

    /// Add features to this dependency.
    pub fn with_features(mut self, features: Vec<String>) -> Self {
        self.features = features;
        self
    }
}

/// Parser for notebook external dependencies.
pub struct DependencyParser {
    dependencies: Vec<ExternalDependency>,
}

impl DependencyParser {
    /// Create a new dependency parser.
    pub fn new() -> Self {
        Self {
            dependencies: Vec::new(),
        }
    }

    /// Parse dependencies from notebook source.
    ///
    /// Looks for a block like:
    /// ```text
    /// //! ```cargo
    /// //! [dependencies]
    /// //! serde = "1.0"
    /// //! tokio = { version = "1", features = ["full"] }
    /// //! ```
    /// ```
    pub fn parse(&mut self, source: &str) -> &[ExternalDependency] {
        self.dependencies.clear();

        let mut in_cargo_block = false;
        let mut in_dependencies = false;
        let mut toml_content = String::new();

        for line in source.lines() {
            let trimmed = line.trim();

            // Check for cargo block markers
            if trimmed.starts_with("//!") {
                let content = trimmed.trim_start_matches("//!").trim();

                if content == "```cargo" {
                    in_cargo_block = true;
                    continue;
                }

                if content == "```" && in_cargo_block {
                    in_cargo_block = false;
                    in_dependencies = false;
                    continue;
                }

                if in_cargo_block {
                    if content == "[dependencies]" {
                        in_dependencies = true;
                        continue;
                    }

                    if content.starts_with('[') {
                        in_dependencies = false;
                        continue;
                    }

                    if in_dependencies && !content.is_empty() {
                        toml_content.push_str(content);
                        toml_content.push('\n');
                    }
                }
            }
        }

        // Parse the TOML content
        if !toml_content.is_empty() {
            self.parse_toml_dependencies(&toml_content);
        }

        &self.dependencies
    }

    /// Get the parsed dependencies.
    pub fn dependencies(&self) -> &[ExternalDependency] {
        &self.dependencies
    }

    /// Calculate a hash of the dependencies for cache invalidation.
    pub fn calculate_hash(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.dependencies.hash(&mut hasher);
        hasher.finish()
    }

    /// Parse TOML-format dependencies.
    fn parse_toml_dependencies(&mut self, toml: &str) {
        for line in toml.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse: name = "version" or name = { version = "...", ... }
            if let Some((name, value)) = line.split_once('=') {
                let name = name.trim().to_string();
                let value = value.trim();

                let dep = if value.starts_with('"') {
                    // Simple version: name = "1.0"
                    let version = value.trim_matches('"').to_string();
                    ExternalDependency {
                        name,
                        version: Some(version),
                        features: Vec::new(),
                        path: None,
                    }
                } else if value.starts_with('{') {
                    // Table format: name = { version = "1.0", features = [...] }
                    Self::parse_table_dependency(name, value)
                } else {
                    continue;
                };

                self.dependencies.push(dep);
            }
        }
    }

    /// Parse a table-format dependency.
    fn parse_table_dependency(name: String, value: &str) -> ExternalDependency {
        let mut version = None;
        let mut features = Vec::new();
        let mut path = None;

        // Simple parser for inline tables
        let content = value.trim_start_matches('{').trim_end_matches('}');

        for part in content.split(',') {
            let part = part.trim();
            if let Some((key, val)) = part.split_once('=') {
                let key = key.trim();
                let val = val.trim();

                match key {
                    "version" => {
                        version = Some(val.trim_matches('"').to_string());
                    }
                    "path" => {
                        path = Some(PathBuf::from(val.trim_matches('"')));
                    }
                    "features" => {
                        // Parse array: ["feat1", "feat2"]
                        let arr = val.trim_start_matches('[').trim_end_matches(']');
                        for feat in arr.split(',') {
                            let feat = feat.trim().trim_matches('"');
                            if !feat.is_empty() {
                                features.push(feat.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        ExternalDependency {
            name,
            version,
            features,
            path,
        }
    }
}

impl Default for DependencyParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_dependency() {
        let mut parser = DependencyParser::new();

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

        let deps = parser.parse(source);

        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "serde");
        assert_eq!(deps[0].version, Some("1.0".to_string()));
    }

    #[test]
    fn test_parse_complex_dependency() {
        let mut parser = DependencyParser::new();

        let source = r#"
//! ```cargo
//! [dependencies]
//! tokio = { version = "1", features = ["full"] }
//! ```
"#;

        let deps = parser.parse(source);

        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "tokio");
        assert_eq!(deps[0].version, Some("1".to_string()));
        assert_eq!(deps[0].features, vec!["full"]);
    }

    #[test]
    fn test_parse_multiple_dependencies() {
        let mut parser = DependencyParser::new();

        let source = r#"
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! serde_json = "1.0"
//! tokio = { version = "1", features = ["rt", "macros"] }
//! ```
"#;

        let deps = parser.parse(source);

        assert_eq!(deps.len(), 3);
    }

    #[test]
    fn test_hash_changes_with_deps() {
        let mut parser = DependencyParser::new();

        parser.parse("");
        let hash1 = parser.calculate_hash();

        parser.parse(
            r#"
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```
"#,
        );
        let hash2 = parser.calculate_hash();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_dependency_builders() {
        let dep = ExternalDependency::simple("serde", "1.0")
            .with_features(vec!["derive".to_string()]);

        assert_eq!(dep.name, "serde");
        assert_eq!(dep.version, Some("1.0".to_string()));
        assert_eq!(dep.features, vec!["derive"]);
    }
}
