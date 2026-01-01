//! Error handling and mapping for the compilation pipeline.

use std::path::PathBuf;

use serde::Deserialize;

/// A compilation error with source location information.
#[derive(Debug, Clone)]
pub struct CompileError {
    /// Error message
    pub message: String,

    /// Error code (e.g., "E0308")
    pub code: Option<String>,

    /// Severity level
    pub level: ErrorLevel,

    /// Primary source location
    pub location: Option<SourceLocation>,

    /// Additional spans (e.g., "help: consider...")
    pub spans: Vec<ErrorSpan>,

    /// Rendered error message (for display)
    pub rendered: Option<String>,
}

/// Severity level of an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorLevel {
    Error,
    Warning,
    Note,
    Help,
}

/// A location in source code.
#[derive(Debug, Clone)]
pub struct SourceLocation {
    /// Source file path
    pub file: PathBuf,

    /// Line number (1-indexed)
    pub line: usize,

    /// Column number (1-indexed)
    pub column: usize,
}

/// An error span with label.
#[derive(Debug, Clone)]
pub struct ErrorSpan {
    /// Location of this span
    pub location: SourceLocation,

    /// End location (for multi-line spans)
    pub end_location: Option<SourceLocation>,

    /// Label for this span
    pub label: Option<String>,

    /// Whether this is the primary span
    pub is_primary: bool,
}

/// Rustc JSON diagnostic format.
#[derive(Debug, Deserialize)]
pub struct RustcDiagnostic {
    pub message: String,
    pub code: Option<RustcCode>,
    pub level: String,
    pub spans: Vec<RustcSpan>,
    pub rendered: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RustcCode {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct RustcSpan {
    #[allow(dead_code)] // Used for multi-file compilation debugging
    pub file_name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub column_start: usize,
    pub column_end: usize,
    pub is_primary: bool,
    pub label: Option<String>,
}

/// Maps rustc errors to original source locations.
pub struct ErrorMapper {
    /// Mapping from generated line to original line
    line_map: Vec<LineMapping>,

    /// Original source file path
    original_file: PathBuf,
}

/// Mapping from generated code line to original source.
#[derive(Debug, Clone)]
struct LineMapping {
    /// Line number in generated code
    generated_line: usize,

    /// Line number in original source
    original_line: usize,

    /// Column offset (if any)
    #[allow(dead_code)] // Reserved for future column-level error mapping
    column_offset: isize,
}

impl ErrorMapper {
    /// Create a new error mapper for a source file.
    pub fn new(original_file: PathBuf) -> Self {
        Self {
            line_map: Vec::new(),
            original_file,
        }
    }

    /// Add a line mapping from generated code to original source.
    pub fn add_mapping(&mut self, generated_line: usize, original_line: usize) {
        self.line_map.push(LineMapping {
            generated_line,
            original_line,
            column_offset: 0,
        });
    }

    /// Parse rustc JSON output and map errors to original locations.
    pub fn parse_rustc_output(&self, json_output: &str) -> Vec<CompileError> {
        let mut errors = Vec::new();

        for line in json_output.lines() {
            if line.trim().is_empty() {
                continue;
            }

            // Try to parse as JSON diagnostic
            match serde_json::from_str::<RustcDiagnostic>(line) {
                Ok(diagnostic) => {
                    if let Some(error) = self.map_diagnostic(&diagnostic) {
                        errors.push(error);
                    }
                }
                Err(e) => {
                    // Log parse failures to aid debugging malformed rustc output
                    tracing::debug!(
                        "Failed to parse rustc JSON: {} (line: {})",
                        e,
                        if line.len() > 100 { &line[..100] } else { line }
                    );
                }
            }
        }

        errors
    }

    /// Map a rustc diagnostic to a CompileError with corrected locations.
    fn map_diagnostic(&self, diagnostic: &RustcDiagnostic) -> Option<CompileError> {
        let level = match diagnostic.level.as_str() {
            "error" => ErrorLevel::Error,
            "warning" => ErrorLevel::Warning,
            "note" => ErrorLevel::Note,
            "help" => ErrorLevel::Help,
            _ => return None, // Skip unknown levels
        };

        // Find primary span
        let primary_span = diagnostic.spans.iter().find(|s| s.is_primary);

        let location = primary_span.map(|span| self.map_location(span));

        let spans: Vec<ErrorSpan> = diagnostic
            .spans
            .iter()
            .map(|span| ErrorSpan {
                location: self.map_location(span),
                end_location: if span.line_start != span.line_end {
                    Some(SourceLocation {
                        file: self.original_file.clone(),
                        line: self.map_line(span.line_end),
                        column: span.column_end,
                    })
                } else {
                    None
                },
                label: span.label.clone(),
                is_primary: span.is_primary,
            })
            .collect();

        Some(CompileError {
            message: diagnostic.message.clone(),
            code: diagnostic.code.as_ref().map(|c| c.code.clone()),
            level,
            location,
            spans,
            rendered: diagnostic.rendered.clone(),
        })
    }

    /// Map a rustc span to a source location.
    fn map_location(&self, span: &RustcSpan) -> SourceLocation {
        SourceLocation {
            file: self.original_file.clone(),
            line: self.map_line(span.line_start),
            column: span.column_start,
        }
    }

    /// Map a generated line number to original line number.
    fn map_line(&self, generated_line: usize) -> usize {
        // Find the mapping entry for this line
        for mapping in &self.line_map {
            if mapping.generated_line == generated_line {
                return mapping.original_line;
            }
        }

        // If no mapping found, use a heuristic based on nearest mapping
        if let Some(mapping) = self
            .line_map
            .iter()
            .min_by_key(|m| (m.generated_line as isize - generated_line as isize).unsigned_abs())
        {
            let offset = generated_line as isize - mapping.generated_line as isize;
            return (mapping.original_line as isize + offset).max(1) as usize;
        }

        // No mappings at all - return as-is
        generated_line
    }
}

impl CompileError {
    /// Create a simple error with just a message.
    pub fn simple(message: impl Into<String>) -> Vec<Self> {
        vec![Self {
            message: message.into(),
            code: None,
            level: ErrorLevel::Error,
            location: None,
            spans: Vec::new(),
            rendered: None,
        }]
    }

    /// Create a simple error with a pre-rendered message (for raw rustc output).
    pub fn simple_rendered(message: impl Into<String>) -> Vec<Self> {
        let msg = message.into();
        vec![Self {
            message: msg.clone(),
            code: None,
            level: ErrorLevel::Error,
            location: None,
            spans: Vec::new(),
            rendered: Some(msg),
        }]
    }

    /// Format the error for terminal display.
    pub fn format_terminal(&self) -> String {
        let mut output = String::new();

        // Level and message
        let level_str = match self.level {
            ErrorLevel::Error => "\x1b[1;31merror\x1b[0m",
            ErrorLevel::Warning => "\x1b[1;33mwarning\x1b[0m",
            ErrorLevel::Note => "\x1b[1;36mnote\x1b[0m",
            ErrorLevel::Help => "\x1b[1;32mhelp\x1b[0m",
        };

        if let Some(code) = &self.code {
            output.push_str(&format!("{level_str}[{code}]: {}\n", self.message));
        } else {
            output.push_str(&format!("{level_str}: {}\n", self.message));
        }

        // Location
        if let Some(loc) = &self.location {
            output.push_str(&format!(
                "  \x1b[1;34m-->\x1b[0m {}:{}:{}\n",
                loc.file.display(),
                loc.line,
                loc.column
            ));
        }

        output
    }

    /// Format the error for JSON output.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "message": self.message,
            "code": self.code,
            "level": format!("{:?}", self.level).to_lowercase(),
            "location": self.location.as_ref().map(|loc| {
                serde_json::json!({
                    "file": loc.file.display().to_string(),
                    "line": loc.line,
                    "column": loc.column,
                })
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rustc_json() {
        let json = r#"{"message":"expected type, found `42`","code":{"code":"E0573"},"level":"error","spans":[{"file_name":"test.rs","line_start":5,"line_end":5,"column_start":10,"column_end":12,"is_primary":true,"label":"expected type"}],"rendered":"error[E0573]: expected type, found `42`"}"#;

        let mapper = ErrorMapper::new(PathBuf::from("test.rs"));
        let errors = mapper.parse_rustc_output(json);

        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, Some("E0573".to_string()));
        assert_eq!(errors[0].level, ErrorLevel::Error);
        assert!(errors[0].message.contains("expected type"));
    }

    #[test]
    fn test_line_mapping() {
        let mut mapper = ErrorMapper::new(PathBuf::from("original.rs"));
        mapper.add_mapping(10, 5); // Generated line 10 = original line 5
        mapper.add_mapping(20, 15); // Generated line 20 = original line 15

        assert_eq!(mapper.map_line(10), 5);
        assert_eq!(mapper.map_line(20), 15);
        // Interpolate for lines in between
        assert_eq!(mapper.map_line(15), 10);
    }

    #[test]
    fn test_error_format() {
        let error = CompileError {
            message: "test error".to_string(),
            code: Some("E0001".to_string()),
            level: ErrorLevel::Error,
            location: Some(SourceLocation {
                file: PathBuf::from("test.rs"),
                line: 10,
                column: 5,
            }),
            spans: Vec::new(),
            rendered: None,
        };

        let formatted = error.format_terminal();
        assert!(formatted.contains("error"));
        assert!(formatted.contains("E0001"));
        assert!(formatted.contains("test.rs:10:5"));
    }
}
