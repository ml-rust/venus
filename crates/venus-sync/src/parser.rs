//! Parser for Venus `.rs` notebooks.
//!
//! Extracts cells, doc comments, and metadata from `.rs` files.

use std::fs;
use std::path::Path;

use crate::error::{SyncError, SyncResult};

/// Metadata extracted from the notebook header.
#[derive(Debug, Clone, Default)]
pub struct NotebookMetadata {
    /// Notebook title (from first `# Title` in doc comment)
    pub title: Option<String>,

    /// Notebook description (from doc comment after title)
    pub description: Option<String>,

    /// Dependencies (parsed from `//! ```cargo` block)
    pub dependencies: Vec<String>,
}

/// A cell extracted from the notebook.
#[derive(Debug, Clone)]
pub struct NotebookCell {
    /// Cell name (function name)
    pub name: String,

    /// Cell type
    pub cell_type: CellType,

    /// Markdown content (for markdown cells or doc comments)
    pub markdown: Option<String>,

    /// Rust source code (for code cells)
    pub source: Option<String>,

    /// Whether this cell has dependencies
    pub has_dependencies: bool,
}

/// Type of cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellType {
    /// Markdown documentation cell
    Markdown,
    /// Code cell with executable Rust
    Code,
}

/// Parser for `.rs` Venus notebooks.
pub struct RsParser {
    // Reserved for future configuration
}

impl RsParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self {}
    }

    /// Parse a `.rs` file into notebook metadata and cells.
    pub fn parse_file(
        &self,
        path: impl AsRef<Path>,
    ) -> SyncResult<(NotebookMetadata, Vec<NotebookCell>)> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|e| SyncError::ReadError {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        self.parse_source(&source)
    }

    /// Parse source code into notebook metadata and cells.
    pub fn parse_source(&self, source: &str) -> SyncResult<(NotebookMetadata, Vec<NotebookCell>)> {
        let mut metadata = NotebookMetadata::default();
        let mut cells = Vec::new();

        // First pass: extract module-level doc comments for metadata
        let mut in_cargo_block = false;
        let mut header_lines = Vec::new();

        for line in source.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("//!") {
                let content = trimmed.trim_start_matches("//!").trim();

                if content == "```cargo" {
                    in_cargo_block = true;
                    continue;
                }

                if content == "```" && in_cargo_block {
                    in_cargo_block = false;
                    continue;
                }

                if in_cargo_block {
                    // Skip cargo block content for markdown
                    if content.starts_with('[') || content.contains('=') {
                        continue;
                    }
                }

                header_lines.push(content.to_string());
            } else if !trimmed.is_empty() && !trimmed.starts_with("//") {
                break;
            }
        }

        // Parse header for title and description
        if !header_lines.is_empty() {
            let first = &header_lines[0];
            if first.starts_with("# ") {
                metadata.title = Some(first.trim_start_matches("# ").to_string());
                if header_lines.len() > 1 {
                    // Skip empty lines after title
                    let desc_lines: Vec<&String> = header_lines[1..]
                        .iter()
                        .skip_while(|l| l.is_empty())
                        .collect();
                    if !desc_lines.is_empty() {
                        metadata.description = Some(
                            desc_lines
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                    }
                }
            }
        }

        // Create a markdown cell for the header if there's content
        let header_md = self.extract_header_markdown(source);
        if let Some(md) = header_md {
            cells.push(NotebookCell {
                name: "_header".to_string(),
                cell_type: CellType::Markdown,
                markdown: Some(md),
                source: None,
                has_dependencies: false,
            });
        }

        // Second pass: extract cells
        self.extract_cells(source, &mut cells)?;

        Ok((metadata, cells))
    }

    /// Extract the header markdown from module doc comments.
    fn extract_header_markdown(&self, source: &str) -> Option<String> {
        let mut lines = Vec::new();
        let mut in_cargo_block = false;

        for line in source.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("//!") {
                let content = trimmed.trim_start_matches("//!").trim_start();

                if content == "```cargo" {
                    in_cargo_block = true;
                    continue;
                }

                if content == "```" && in_cargo_block {
                    in_cargo_block = false;
                    continue;
                }

                if !in_cargo_block {
                    lines.push(content.to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with("//") {
                break;
            }
        }

        if lines.is_empty() || lines.iter().all(|l| l.is_empty()) {
            None
        } else {
            // Trim trailing empty lines
            while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
                lines.pop();
            }
            Some(lines.join("\n"))
        }
    }

    /// Extract cells from source code.
    fn extract_cells(&self, source: &str, cells: &mut Vec<NotebookCell>) -> SyncResult<()> {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            // Look for #[venus::cell] attribute
            if line == "#[venus::cell]" || line.starts_with("#[venus::cell(") {
                // Collect doc comment before the attribute
                let doc_start = self.find_doc_comment_start(&lines, i);
                let doc_comment = if doc_start < i {
                    Some(self.extract_doc_comment(&lines[doc_start..i]))
                } else {
                    None
                };

                // Find the function
                i += 1;
                while i < lines.len() && lines[i].trim().is_empty() {
                    i += 1;
                }

                if i >= lines.len() {
                    break;
                }

                // Extract function
                let fn_line = lines[i].trim();
                if !fn_line.starts_with("pub fn ") && !fn_line.starts_with("fn ") {
                    i += 1;
                    continue;
                }

                // Get function name and check for dependencies
                let (name, has_deps) = self.parse_function_signature(fn_line);

                // Find the end of the function (matching braces)
                let fn_start = i;
                let fn_end = self.find_function_end(&lines, i);

                // Build the source (include attribute and function)
                let attr_line = if doc_start < fn_start - 1 {
                    lines[fn_start - 1].trim()
                } else {
                    "#[venus::cell]"
                };

                let mut source_lines = vec![attr_line.to_string()];
                for line in lines.iter().take(fn_end + 1).skip(fn_start) {
                    source_lines.push((*line).to_string());
                }
                let source_code = source_lines.join("\n");

                // Add markdown cell for doc comment if present
                if let Some(md) = &doc_comment
                    && !md.is_empty()
                {
                    cells.push(NotebookCell {
                        name: format!("{}_doc", name),
                        cell_type: CellType::Markdown,
                        markdown: Some(md.clone()),
                        source: None,
                        has_dependencies: false,
                    });
                }

                // Add code cell
                cells.push(NotebookCell {
                    name: name.clone(),
                    cell_type: CellType::Code,
                    markdown: None,
                    source: Some(source_code),
                    has_dependencies: has_deps,
                });

                i = fn_end + 1;
            } else {
                i += 1;
            }
        }

        Ok(())
    }

    /// Find the start of doc comments before a given line.
    fn find_doc_comment_start(&self, lines: &[&str], attr_line: usize) -> usize {
        if attr_line == 0 {
            return attr_line;
        }

        let mut start = attr_line;
        for i in (0..attr_line).rev() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with("///") || trimmed.is_empty() {
                start = i;
            } else {
                break;
            }
        }

        // Skip leading empty lines
        while start < attr_line && lines[start].trim().is_empty() {
            start += 1;
        }

        start
    }

    /// Extract doc comment content from lines.
    fn extract_doc_comment(&self, lines: &[&str]) -> String {
        lines
            .iter()
            .filter(|l| l.trim().starts_with("///"))
            .map(|l| {
                let content = l.trim().trim_start_matches("///");
                // Remove leading space if present
                content.strip_prefix(' ').unwrap_or(content)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Parse function signature to extract name and check for dependencies.
    fn parse_function_signature(&self, line: &str) -> (String, bool) {
        // Remove visibility and fn keyword
        let stripped = line
            .trim_start_matches("pub ")
            .trim_start_matches("fn ")
            .trim();

        // Extract name (before the '(')
        let name = stripped
            .split('(')
            .next()
            .unwrap_or("unknown")
            .trim()
            .to_string();

        // Check for dependencies (non-empty parameter list)
        let has_deps = if let Some(params_start) = stripped.find('(') {
            if let Some(params_end) = stripped.find(')') {
                let params = &stripped[params_start + 1..params_end].trim();
                !params.is_empty()
                    && *params != "&mut self"
                    && *params != "&self"
                    && *params != "self"
            } else {
                false
            }
        } else {
            false
        };

        (name, has_deps)
    }

    /// Find the end of a function (matching closing brace).
    fn find_function_end(&self, lines: &[&str], start: usize) -> usize {
        let mut brace_count = 0;
        let mut found_open = false;

        for (i, line) in lines.iter().enumerate().skip(start) {
            for c in line.chars() {
                if c == '{' {
                    brace_count += 1;
                    found_open = true;
                } else if c == '}' {
                    brace_count -= 1;
                    if found_open && brace_count == 0 {
                        return i;
                    }
                }
            }
        }

        // If no end found, return the last line
        lines.len().saturating_sub(1)
    }
}

impl Default for RsParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_notebook() {
        let source = r#"//! # My Notebook
//!
//! A test notebook.

use venus::prelude::*;

/// Returns a greeting.
#[venus::cell]
pub fn hello() -> String {
    "Hello, Venus!".to_string()
}
"#;

        let parser = RsParser::new();
        let (metadata, cells) = parser.parse_source(source).unwrap();

        assert_eq!(metadata.title, Some("My Notebook".to_string()));
        assert!(metadata.description.is_some());

        // Should have header markdown + doc + code cell
        assert!(cells.len() >= 2);

        // Find the code cell
        let code_cell = cells
            .iter()
            .find(|c| c.cell_type == CellType::Code)
            .unwrap();
        assert_eq!(code_cell.name, "hello");
        assert!(!code_cell.has_dependencies);
    }

    #[test]
    fn test_parse_cell_with_dependencies() {
        let source = r#"
#[venus::cell]
pub fn process(data: &String) -> i32 {
    data.len() as i32
}
"#;

        let parser = RsParser::new();
        let (_, cells) = parser.parse_source(source).unwrap();

        let code_cell = cells
            .iter()
            .find(|c| c.cell_type == CellType::Code)
            .unwrap();
        assert_eq!(code_cell.name, "process");
        assert!(code_cell.has_dependencies);
    }

    #[test]
    fn test_extract_doc_comment() {
        let source = r#"
/// This is a doc comment.
/// It has multiple lines.
#[venus::cell]
pub fn example() -> i32 { 42 }
"#;

        let parser = RsParser::new();
        let (_, cells) = parser.parse_source(source).unwrap();

        // Should have markdown cell from doc comment
        let md_cell = cells
            .iter()
            .find(|c| c.cell_type == CellType::Markdown)
            .unwrap();
        assert!(md_cell.markdown.as_ref().unwrap().contains("doc comment"));
    }

    #[test]
    fn test_function_signature_parsing() {
        let parser = RsParser::new();

        let (name, has_deps) = parser.parse_function_signature("pub fn hello() -> String {");
        assert_eq!(name, "hello");
        assert!(!has_deps);

        let (name, has_deps) =
            parser.parse_function_signature("fn process(data: &Config) -> Output {");
        assert_eq!(name, "process");
        assert!(has_deps);
    }
}
