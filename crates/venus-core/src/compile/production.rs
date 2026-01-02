//! Production builder for Venus notebooks.
//!
//! Generates standalone binaries that execute all cells.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::graph::{CellInfo, CellParser, GraphEngine};

use super::cargo_generator::{generate_cargo_toml, ManifestConfig, ReleaseProfile};
use super::dependency_parser::DependencyParser;
use super::source_processor::NotebookSourceProcessor;
use super::CompilerConfig;

/// Builder for standalone production binaries.
///
/// Unlike interactive execution (via `venus run` or `venus serve`), production
/// builds do not use [`crate::state::StateManager`] since the output is a
/// self-contained executable with its own execution flow.
pub struct ProductionBuilder {
    /// Compiler configuration
    config: CompilerConfig,

    /// Parsed cells
    cells: Vec<CellInfo>,

    /// Dependency graph
    graph: GraphEngine,

    /// Dependency parser
    parser: DependencyParser,

    /// Original notebook source
    source: String,

    /// Notebook file path (for error messages)
    notebook_path: PathBuf,
}

impl ProductionBuilder {
    /// Create a new production builder.
    pub fn new(config: CompilerConfig) -> Self {
        Self {
            config,
            cells: Vec::new(),
            graph: GraphEngine::new(),
            parser: DependencyParser::new(),
            source: String::new(),
            notebook_path: PathBuf::new(),
        }
    }

    /// Load a notebook from the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be read
    /// - The notebook cannot be parsed
    /// - There are duplicate cell names
    /// - There are cyclic dependencies
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        self.notebook_path = path.to_path_buf();
        self.source = fs::read_to_string(path)?;

        // Parse cells
        let mut parser = CellParser::new();
        let parse_result = parser.parse_file(path)?;
        self.cells = parse_result.code_cells;

        // Validate unique cell names
        self.validate_unique_cell_names()?;

        // Build dependency graph
        self.graph = GraphEngine::new();
        for cell in &mut self.cells {
            let real_id = self.graph.add_cell(cell.clone());
            cell.id = real_id;
        }
        self.graph.resolve_dependencies()?;

        // Parse external dependencies
        self.parser.parse(&self.source);

        Ok(())
    }

    /// Validate that all cell names are unique.
    fn validate_unique_cell_names(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for cell in &self.cells {
            if !seen.insert(&cell.name) {
                return Err(Error::Compilation {
                    cell_id: Some(cell.name.clone()),
                    message: format!(
                        "Duplicate cell name '{}' in notebook '{}'",
                        cell.name,
                        self.notebook_path.display()
                    ),
                });
            }
        }
        Ok(())
    }

    /// Build a standalone binary.
    ///
    /// # Arguments
    ///
    /// * `output_path` - Path for the output binary
    /// * `release` - Whether to build with optimizations
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The build directory cannot be created
    /// - Cargo fails to compile the project
    /// - The binary cannot be copied to the output path
    pub fn build(&self, output_path: impl AsRef<Path>, release: bool) -> Result<PathBuf> {
        let output_path = output_path.as_ref();
        let build_dir = self.config.build_dir.join("production");

        fs::create_dir_all(&build_dir)?;

        // Generate Cargo.toml
        let cargo_toml = self.generate_cargo_toml()?;
        fs::write(build_dir.join("Cargo.toml"), cargo_toml)?;

        // Generate main.rs
        let main_rs = self.generate_main_rs()?;
        let src_dir = build_dir.join("src");
        fs::create_dir_all(&src_dir)?;
        fs::write(src_dir.join("main.rs"), main_rs)?;

        // Build with cargo
        let mut cmd = Command::new("cargo");
        cmd.current_dir(&build_dir).arg("build");

        if release {
            cmd.arg("--release");
        }

        // Capture output
        cmd.arg("--message-format=short");

        let output = cmd.output().map_err(|e| Error::Compilation {
            cell_id: None,
            message: format!(
                "Failed to run cargo (working dir: {}): {}",
                build_dir.display(),
                e
            ),
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Compilation {
                cell_id: None,
                message: format!(
                    "Production build failed for '{}':\n{}",
                    self.notebook_path.display(),
                    stderr
                ),
            });
        }

        // Copy the binary to output path
        let profile = if release { "release" } else { "debug" };
        let binary_name = self.binary_name();
        let built_binary = build_dir
            .join("target")
            .join(profile)
            .join(&binary_name);

        fs::copy(&built_binary, output_path)?;

        // Make executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(output_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(output_path, perms)?;
        }

        Ok(output_path.to_path_buf())
    }

    /// Get the number of cells.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Get the dependency count.
    pub fn dependency_count(&self) -> usize {
        self.parser.dependencies().len()
    }

    /// Generate Cargo.toml for the production binary.
    fn generate_cargo_toml(&self) -> Result<String> {
        // Derive binary name from notebook filename
        let name = self
            .notebook_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("notebook")
            .replace('-', "_");

        // Get the notebook directory for resolving relative paths
        let notebook_dir = self.notebook_path.parent().ok_or_else(|| Error::Compilation {
            cell_id: None,
            message: format!(
                "Could not determine parent directory for notebook: {}",
                self.notebook_path.display()
            ),
        })?;

        // Validate all path dependencies can be resolved
        for dep in self.parser.dependencies() {
            if let Some(path) = &dep.path {
                if path.is_relative() {
                    let full_path = notebook_dir.join(path);
                    full_path.canonicalize().map_err(|e| Error::Compilation {
                        cell_id: None,
                        message: format!(
                            "Failed to resolve path dependency '{}' ({}): {}",
                            dep.name,
                            full_path.display(),
                            e
                        ),
                    })?;
                }
            }
        }

        let config = ManifestConfig {
            name: &name,
            version: "0.1.0",
            edition: "2021",
            lib_crate_types: None,
            release_profile: Some(ReleaseProfile::production()),
            standalone_workspace: true,
        };

        Ok(generate_cargo_toml(
            &config,
            self.parser.dependencies(),
            true, // Always include serde for consistency
            Some(notebook_dir),
        ))
    }

    /// Generate main.rs with all cells and execution logic.
    fn generate_main_rs(&self) -> Result<String> {
        let mut code = String::new();

        // Header
        code.push_str("//! Generated by Venus - standalone notebook binary.\n");
        code.push_str("//!\n");
        code.push_str(&format!(
            "//! Source: {}\n",
            self.notebook_path.display()
        ));
        code.push_str("\n");
        code.push_str("#![allow(unused_imports)]\n");
        code.push_str("#![allow(dead_code)]\n");
        code.push_str("#![allow(clippy::ptr_arg)]\n");
        code.push('\n');

        // Process source using proper syn-based parsing
        let processed_source =
            NotebookSourceProcessor::process_for_production(&self.source).map_err(|e| {
                Error::Compilation {
                    cell_id: None,
                    message: format!(
                        "Failed to parse notebook source '{}': {}",
                        self.notebook_path.display(),
                        e
                    ),
                }
            })?;
        code.push_str(&processed_source);
        code.push('\n');

        // Main function
        code.push_str("fn main() {\n");
        code.push_str("    println!(\"═══════════════════════════════════════════════════\");\n");
        code.push_str(&format!(
            "    println!(\"  Venus Notebook: {}\");\n",
            self.notebook_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("notebook")
        ));
        code.push_str("    println!(\"═══════════════════════════════════════════════════\");\n");
        code.push_str("    println!();\n\n");

        // Execute cells in topological order
        let order = self.graph.topological_order()?;

        for cell_id in &order {
            let cell = self
                .cells
                .iter()
                .find(|c| c.id == *cell_id)
                .ok_or_else(|| Error::CellNotFound(format!("{:?}", cell_id)))?;

            // Build call arguments
            let args: Vec<String> = cell
                .dependencies
                .iter()
                .map(|dep| {
                    if dep.is_ref {
                        if dep.is_mut {
                            format!("&mut {}", dep.param_name)
                        } else {
                            format!("&{}", dep.param_name)
                        }
                    } else {
                        dep.param_name.clone()
                    }
                })
                .collect();

            // Generate cell execution
            code.push_str(&format!("    println!(\"▶ Running: {}\");\n", cell.name));

            // Call the cell function and store result
            code.push_str(&format!(
                "    let {} = {}({});\n",
                cell.name,
                cell.name,
                args.join(", ")
            ));

            // Print output
            code.push_str(&format!(
                "    println!(\"  → {{:?}}\", {});\n",
                cell.name
            ));
            code.push_str("    println!();\n");
        }

        code.push_str("    println!(\"═══════════════════════════════════════════════════\");\n");
        code.push_str(&format!(
            "    println!(\"  Completed {} cell(s)\");\n",
            order.len()
        ));
        code.push_str("    println!(\"═══════════════════════════════════════════════════\");\n");
        code.push_str("}\n");

        Ok(code)
    }

    /// Get the platform-specific binary name.
    fn binary_name(&self) -> String {
        let name = self
            .notebook_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("notebook")
            .replace('-', "_");

        #[cfg(target_os = "windows")]
        {
            format!("{}.exe", name)
        }
        #[cfg(not(target_os = "windows"))]
        {
            name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_name() {
        let config = CompilerConfig::default();
        let mut builder = ProductionBuilder::new(config);
        builder.notebook_path = PathBuf::from("my-notebook.rs");

        let name = builder.binary_name();

        #[cfg(target_os = "windows")]
        assert_eq!(name, "my_notebook.exe");

        #[cfg(not(target_os = "windows"))]
        assert_eq!(name, "my_notebook");
    }

    #[test]
    fn test_validate_unique_cell_names() {
        use crate::graph::{CellId, SourceSpan};

        let config = CompilerConfig::default();
        let mut builder = ProductionBuilder::new(config);
        builder.notebook_path = PathBuf::from("test.rs");

        let span = SourceSpan {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 10,
        };

        // Unique names should pass
        builder.cells = vec![
            CellInfo {
                id: CellId::new(0),
                name: "foo".to_string(),
                display_name: "foo".to_string(),
                dependencies: vec![],
                return_type: "i32".to_string(),
                doc_comment: None,
                source_code: String::new(),
                source_file: PathBuf::new(),
                span: span.clone(),
            },
            CellInfo {
                id: CellId::new(1),
                name: "bar".to_string(),
                display_name: "bar".to_string(),
                dependencies: vec![],
                return_type: "i32".to_string(),
                doc_comment: None,
                source_code: String::new(),
                source_file: PathBuf::new(),
                span: span.clone(),
            },
        ];
        assert!(builder.validate_unique_cell_names().is_ok());

        // Duplicate names should fail
        builder.cells.push(CellInfo {
            id: CellId::new(2),
            name: "foo".to_string(), // Duplicate!
            display_name: "foo".to_string(),
            dependencies: vec![],
            return_type: "i32".to_string(),
            doc_comment: None,
            source_code: String::new(),
            source_file: PathBuf::new(),
            span,
        });
        assert!(builder.validate_unique_cell_names().is_err());
    }
}
