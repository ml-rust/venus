//! Jupyter notebook (.ipynb) generation.
//!
//! Converts Venus cells to Jupyter notebook format.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{SyncError, SyncResult};
use crate::outputs::OutputCache;
use crate::parser::{CellType, NotebookCell, NotebookMetadata};

/// A Jupyter notebook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupyterNotebook {
    /// Notebook metadata
    pub metadata: JupyterMetadata,

    /// Format version (always 4)
    pub nbformat: u32,

    /// Minor format version
    pub nbformat_minor: u32,

    /// Notebook cells
    pub cells: Vec<JupyterCell>,
}

/// Jupyter notebook metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupyterMetadata {
    /// Kernel specification
    pub kernelspec: KernelSpec,

    /// Language info
    pub language_info: LanguageInfo,

    /// Venus-specific metadata for round-trip
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venus: Option<VenusMetadata>,
}

/// Kernel specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelSpec {
    /// Display name
    pub display_name: String,

    /// Language
    pub language: String,

    /// Kernel name
    pub name: String,
}

/// Language information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageInfo {
    /// File extension
    pub file_extension: String,

    /// MIME type
    pub mimetype: String,

    /// Language name
    pub name: String,

    /// Version
    pub version: String,
}

/// Venus-specific metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenusMetadata {
    /// Source file path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,

    /// Venus version
    pub version: String,
}

/// A Jupyter cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JupyterCell {
    /// Cell type
    pub cell_type: String,

    /// Cell metadata
    pub metadata: CellMetadata,

    /// Cell source (lines)
    pub source: Vec<String>,

    /// Cell outputs (for code cells)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<CellOutput>>,

    /// Execution count (for code cells)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_count: Option<u32>,
}

/// Cell metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellMetadata {
    /// Venus cell name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venus_cell: Option<String>,

    /// Whether the cell is editable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editable: Option<bool>,

    /// Tags
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

/// Cell output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "output_type")]
pub enum CellOutput {
    /// Standard output/error
    #[serde(rename = "stream")]
    Stream { name: String, text: Vec<String> },

    /// Rich display data
    #[serde(rename = "execute_result")]
    ExecuteResult {
        execution_count: u32,
        data: OutputData,
        metadata: serde_json::Value,
    },

    /// Display data
    #[serde(rename = "display_data")]
    DisplayData {
        data: OutputData,
        metadata: serde_json::Value,
    },

    /// Error output
    #[serde(rename = "error")]
    Error {
        ename: String,
        evalue: String,
        traceback: Vec<String>,
    },
}

/// Output data with multiple representations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputData {
    /// Plain text
    #[serde(rename = "text/plain", skip_serializing_if = "Option::is_none")]
    pub text_plain: Option<Vec<String>>,

    /// HTML
    #[serde(rename = "text/html", skip_serializing_if = "Option::is_none")]
    pub text_html: Option<Vec<String>>,

    /// PNG image (base64)
    #[serde(rename = "image/png", skip_serializing_if = "Option::is_none")]
    pub image_png: Option<String>,

    /// SVG image
    #[serde(rename = "image/svg+xml", skip_serializing_if = "Option::is_none")]
    pub image_svg: Option<Vec<String>>,

    /// JSON data
    #[serde(rename = "application/json", skip_serializing_if = "Option::is_none")]
    pub application_json: Option<serde_json::Value>,
}

impl JupyterNotebook {
    /// Create a new empty notebook.
    pub fn new() -> Self {
        Self {
            metadata: JupyterMetadata::default(),
            nbformat: 4,
            nbformat_minor: 5,
            cells: Vec::new(),
        }
    }

    /// Write the notebook to a file.
    pub fn write_to_file(&self, path: impl AsRef<Path>) -> SyncResult<()> {
        let path = path.as_ref();
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json).map_err(|e| SyncError::WriteError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        Ok(())
    }

    /// Read a notebook from a file.
    pub fn read_from_file(path: impl AsRef<Path>) -> SyncResult<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|e| SyncError::ReadError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let notebook: Self = serde_json::from_str(&content)?;
        Ok(notebook)
    }
}

impl Default for JupyterNotebook {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for JupyterMetadata {
    fn default() -> Self {
        Self {
            kernelspec: KernelSpec {
                display_name: "Rust (Venus)".to_string(),
                language: "rust".to_string(),
                name: "venus".to_string(),
            },
            language_info: LanguageInfo {
                file_extension: ".rs".to_string(),
                mimetype: "text/rust".to_string(),
                name: "rust".to_string(),
                version: "1.0".to_string(),
            },
            venus: Some(VenusMetadata {
                source_file: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
        }
    }
}

/// Generator for Jupyter notebooks from Venus cells.
pub struct IpynbGenerator {
    /// Execution counter
    execution_count: u32,
}

impl IpynbGenerator {
    /// Create a new generator.
    pub fn new() -> Self {
        Self { execution_count: 1 }
    }

    /// Generate a Jupyter notebook from Venus cells.
    pub fn generate(
        &mut self,
        metadata: &NotebookMetadata,
        cells: &[NotebookCell],
        cache: Option<&OutputCache>,
    ) -> SyncResult<JupyterNotebook> {
        let mut notebook = JupyterNotebook::new();

        // Set metadata
        if let Some(title) = &metadata.title {
            notebook.metadata.venus = Some(VenusMetadata {
                source_file: Some(title.clone()),
                version: env!("CARGO_PKG_VERSION").to_string(),
            });
        }

        // Convert cells
        for cell in cells {
            let jupyter_cell = self.convert_cell(cell, cache)?;
            notebook.cells.push(jupyter_cell);
        }

        Ok(notebook)
    }

    /// Convert a Venus cell to a Jupyter cell.
    fn convert_cell(
        &mut self,
        cell: &NotebookCell,
        cache: Option<&OutputCache>,
    ) -> SyncResult<JupyterCell> {
        match cell.cell_type {
            CellType::Markdown => {
                let source = cell.markdown.as_deref().unwrap_or("");
                Ok(JupyterCell {
                    cell_type: "markdown".to_string(),
                    metadata: CellMetadata {
                        venus_cell: Some(cell.name.clone()),
                        editable: Some(true),
                        tags: None,
                    },
                    source: source.lines().map(|l| format!("{}\n", l)).collect(),
                    outputs: None,
                    execution_count: None,
                })
            }
            CellType::Code => {
                let source = cell.source.as_deref().unwrap_or("");
                let exec_count = self.execution_count;
                self.execution_count += 1;

                // Get cached output if available
                let outputs = if let Some(cache) = cache {
                    cache.get_output(&cell.name).map(|o| vec![o])
                } else {
                    None
                };

                Ok(JupyterCell {
                    cell_type: "code".to_string(),
                    metadata: CellMetadata {
                        venus_cell: Some(cell.name.clone()),
                        editable: Some(true),
                        tags: if cell.has_dependencies {
                            Some(vec!["has-dependencies".to_string()])
                        } else {
                            None
                        },
                    },
                    source: source.lines().map(|l| format!("{}\n", l)).collect(),
                    outputs: Some(outputs.unwrap_or_default()),
                    execution_count: Some(exec_count),
                })
            }
        }
    }
}

impl Default for IpynbGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_notebook() {
        let notebook = JupyterNotebook::new();
        assert_eq!(notebook.nbformat, 4);
        assert!(notebook.cells.is_empty());
    }

    #[test]
    fn test_generate_markdown_cell() {
        let mut generator = IpynbGenerator::new();

        let cell = NotebookCell {
            name: "intro".to_string(),
            cell_type: CellType::Markdown,
            markdown: Some("# Hello\n\nThis is a test.".to_string()),
            source: None,
            has_dependencies: false,
        };

        let jupyter_cell = generator.convert_cell(&cell, None).unwrap();

        assert_eq!(jupyter_cell.cell_type, "markdown");
        assert!(jupyter_cell.source.len() >= 2);
        assert!(jupyter_cell.outputs.is_none());
    }

    #[test]
    fn test_generate_code_cell() {
        let mut generator = IpynbGenerator::new();

        let cell = NotebookCell {
            name: "compute".to_string(),
            cell_type: CellType::Code,
            markdown: None,
            source: Some("#[venus::cell]\npub fn compute() -> i32 { 42 }".to_string()),
            has_dependencies: false,
        };

        let jupyter_cell = generator.convert_cell(&cell, None).unwrap();

        assert_eq!(jupyter_cell.cell_type, "code");
        assert!(jupyter_cell.execution_count.is_some());
        assert!(jupyter_cell.outputs.is_some());
    }

    #[test]
    fn test_notebook_serialization() {
        let notebook = JupyterNotebook::new();
        let json = serde_json::to_string_pretty(&notebook).unwrap();

        assert!(json.contains("nbformat"));
        assert!(json.contains("metadata"));
        assert!(json.contains("cells"));
    }
}
