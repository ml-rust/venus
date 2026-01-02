//! Cell parser using syn to extract cell information from Rust source files.

use std::path::Path;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, File, FnArg, ItemFn, Pat, ReturnType, Type};

use super::types::{CellId, CellInfo, Dependency, MarkdownCell, SourceSpan};
use crate::error::{Error, Result};

/// Result of parsing a notebook file.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Code cells (executable functions with #[venus::cell]).
    pub code_cells: Vec<CellInfo>,
    /// Markdown cells (documentation blocks).
    pub markdown_cells: Vec<MarkdownCell>,
}

/// Parser for extracting cells from Rust source files.
pub struct CellParser {
    /// Extracted code cells
    cells: Vec<CellInfo>,
    /// Extracted markdown cells
    markdown_cells: Vec<MarkdownCell>,
    /// Source file path
    source_file: std::path::PathBuf,
    /// Source code (for extracting spans)
    source_code: String,
}

impl CellParser {
    /// Create a new parser.
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            markdown_cells: Vec::new(),
            source_file: std::path::PathBuf::new(),
            source_code: String::new(),
        }
    }

    /// Parse a source file and extract all cells.
    pub fn parse_file(&mut self, path: &Path) -> Result<ParseResult> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| Error::Parse(format!("Failed to read file {}: {}", path.display(), e)))?;

        self.parse_str(&source, path)
    }

    /// Parse source code string and extract all cells.
    pub fn parse_str(&mut self, source: &str, path: &Path) -> Result<ParseResult> {
        self.source_file = path.to_path_buf();
        self.source_code = source.to_string();
        self.cells.clear();
        self.markdown_cells.clear();

        let file: File = syn::parse_str(source)
            .map_err(|e| Error::Parse(format!("Failed to parse {}: {}", path.display(), e)))?;

        // Extract module-level doc comments (//!)
        self.extract_module_docs(&file);

        // Visit all items in the file to find code cells
        self.visit_file(&file);

        // Extract standalone // comment blocks (not attached to cells)
        self.extract_standalone_doc_comments(source);

        Ok(ParseResult {
            code_cells: std::mem::take(&mut self.cells),
            markdown_cells: std::mem::take(&mut self.markdown_cells),
        })
    }

    /// Check if a function has the #[venus::cell] attribute.
    fn has_cell_attribute(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|attr| {
            let path = attr.path();
            let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();

            // Match #[venus::cell] or #[cell] (if imported)
            (segments.len() == 2 && segments[0] == "venus" && segments[1] == "cell")
                || (segments.len() == 1 && segments[0] == "cell")
        })
    }

    /// Extract doc comments from attributes.
    fn extract_doc_comment(attrs: &[Attribute]) -> Option<String> {
        let docs: Vec<String> = attrs
            .iter()
            .filter_map(|attr| {
                if attr.path().is_ident("doc")
                    && let syn::Meta::NameValue(nv) = &attr.meta
                    && let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = &nv.value
                {
                    return Some(s.value());
                }
                None
            })
            .collect();

        if docs.is_empty() {
            None
        } else {
            // Join doc lines and trim leading space (Rust adds a space after ///)
            Some(
                docs.iter()
                    .map(|s| s.strip_prefix(' ').unwrap_or(s))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
    }

    /// Extract display name from doc comment.
    ///
    /// Looks for the first markdown heading (# Display Name) in the doc comment.
    /// If not found, returns the function name as fallback.
    fn extract_display_name(doc_comment: &Option<String>, function_name: &str) -> String {
        if let Some(doc) = doc_comment {
            for line in doc.lines() {
                let trimmed = line.trim();
                if let Some(heading) = trimmed.strip_prefix('#') {
                    let display_name = heading.trim();
                    if !display_name.is_empty() {
                        return display_name.to_string();
                    }
                }
            }
        }
        // Fallback to function name
        function_name.to_string()
    }

    /// Extract the type as a string.
    fn type_to_string(ty: &Type) -> String {
        quote::quote!(#ty).to_string()
    }

    /// Extract dependency information from a function parameter.
    fn extract_dependency(arg: &FnArg) -> Option<Dependency> {
        match arg {
            FnArg::Typed(pat_type) => {
                // Extract parameter name
                let param_name = match &*pat_type.pat {
                    Pat::Ident(ident) => ident.ident.to_string(),
                    _ => return None, // Skip complex patterns
                };

                // Skip special parameters like &mut CellContext
                if param_name == "ctx" || param_name == "_ctx" {
                    return None;
                }

                // Extract type information
                let (base_type, is_ref, is_mut) = match &*pat_type.ty {
                    Type::Reference(ref_type) => {
                        let is_mut = ref_type.mutability.is_some();
                        let inner_type = Self::type_to_string(&ref_type.elem);
                        (inner_type, true, is_mut)
                    }
                    other => (Self::type_to_string(other), false, false),
                };

                Some(Dependency {
                    param_name,
                    param_type: base_type,
                    is_ref,
                    is_mut,
                })
            }
            FnArg::Receiver(_) => None, // Skip self parameters
        }
    }

    /// Extract return type as a string.
    fn extract_return_type(ret: &ReturnType) -> String {
        match ret {
            ReturnType::Default => "()".to_string(),
            ReturnType::Type(_, ty) => Self::type_to_string(ty),
        }
    }

    /// Calculate source span from syn span.
    ///
    /// Note: proc_macro2 span locations are only available with the span-locations feature.
    fn span_to_source_span(&self, span: proc_macro2::Span) -> SourceSpan {
        let start = span.start();
        let end = span.end();

        SourceSpan {
            start_line: start.line,
            start_col: start.column,
            end_line: end.line,
            end_col: end.column,
        }
    }

    /// Extract the source code for a function.
    fn extract_source_code(&self, func: &ItemFn) -> String {
        let span = func.block.brace_token.span.join();
        let start = span.start();
        let end = span.end();

        // Get lines of source code
        let lines: Vec<&str> = self.source_code.lines().collect();

        if start.line == 0 || end.line == 0 || start.line > lines.len() {
            // Fallback: use quote to regenerate the function
            return quote::quote!(#func).to_string();
        }

        // Extract the function source (1-indexed lines)
        let func_lines: Vec<&str> = lines
            .iter()
            .skip(start.line - 1)
            .take(end.line - start.line + 1)
            .copied()
            .collect();

        func_lines.join("\n")
    }

    /// Extract module-level doc comments (`//!`) as markdown cells.
    /// Splits into separate cells when there are blank lines between comment blocks.
    fn extract_module_docs(&mut self, file: &File) {
        let mut current_block: Vec<(String, usize)> = Vec::new(); // (content, line_num)
        let mut first_line = 0;
        let mut last_line_in_block = 0;
        let mut prev_line = 0;

        for attr in &file.attrs {
            // Look for #![doc = "..."] attributes (inner attributes)
            if attr.path().is_ident("doc")
                && matches!(attr.style, syn::AttrStyle::Inner(_))
                && let syn::Meta::NameValue(nv) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
            {
                let line_num = attr.span().start().line;

                // If there's a gap (blank line) between this and previous doc comment, start a new block
                if !current_block.is_empty() && line_num > prev_line + 1 {
                    // Finalize the current block as a markdown cell
                    self.finalize_markdown_block(&current_block, first_line, last_line_in_block);
                    current_block.clear();
                    first_line = line_num;
                }

                if current_block.is_empty() {
                    first_line = line_num;
                }

                current_block.push((s.value(), line_num));
                last_line_in_block = line_num;
                prev_line = line_num;
            }
        }

        // Finalize any remaining block
        if !current_block.is_empty() {
            self.finalize_markdown_block(&current_block, first_line, last_line_in_block);
        }
    }

    /// Finalize a markdown block and add it as a markdown cell.
    fn finalize_markdown_block(&mut self, block: &[(String, usize)], first_line: usize, last_line: usize) {
        // Join doc lines and trim leading space (Rust adds a space after //!)
        let content = block
            .iter()
            .map(|(s, _)| s.strip_prefix(' ').unwrap_or(s))
            .collect::<Vec<_>>()
            .join("\n");

        let span = SourceSpan {
            start_line: first_line,
            start_col: 0,
            end_line: last_line,
            end_col: 0,
        };

        self.markdown_cells.push(MarkdownCell {
            id: CellId::new(0), // Will be reassigned by engine
            content,
            span,
            source_file: self.source_file.clone(),
            is_module_doc: true,
        });
    }

    /// Extract standalone // comment blocks as markdown cells.
    /// Splits on blank lines to create separate cells.
    fn extract_standalone_doc_comments(&mut self, source: &str) {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            // Found a // comment (but not //! or ///)
            if trimmed.starts_with("//") && !trimmed.starts_with("//!") && !trimmed.starts_with("///") {
                let start_line = i + 1; // 1-indexed
                let mut comment_lines = vec![];
                let mut j = i;

                // Collect consecutive // lines (stop at blank line)
                while j < lines.len() {
                    let line_trimmed = lines[j].trim();
                    if line_trimmed.starts_with("//") && !line_trimmed.starts_with("//!") && !line_trimmed.starts_with("///") {
                        let content = line_trimmed.strip_prefix("//").unwrap_or("");
                        let content = content.strip_prefix(' ').unwrap_or(content);
                        comment_lines.push(content.to_string());
                        j += 1;
                    } else {
                        // Stop at blank line or non-comment
                        break;
                    }
                }

                // Only create markdown cell if we have content
                if !comment_lines.is_empty() {
                    let content = comment_lines.join("\n");
                    let end_line = j; // j is the line after the last //

                    let span = SourceSpan {
                        start_line,
                        start_col: 0,
                        end_line,
                        end_col: 0,
                    };

                    self.markdown_cells.push(MarkdownCell {
                        id: CellId::new(0),
                        content,
                        span,
                        source_file: self.source_file.clone(),
                        is_module_doc: false,
                    });
                }

                i = j;
            } else {
                i += 1;
            }
        }
    }
}

impl Default for CellParser {
    fn default() -> Self {
        Self::new()
    }
}

impl<'ast> Visit<'ast> for CellParser {
    fn visit_item_fn(&mut self, func: &'ast ItemFn) {
        // Check if this function has #[venus::cell]
        if !Self::has_cell_attribute(&func.attrs) {
            return;
        }

        // Extract cell information
        let name = func.sig.ident.to_string();

        let dependencies: Vec<Dependency> = func
            .sig
            .inputs
            .iter()
            .filter_map(Self::extract_dependency)
            .collect();

        let return_type = Self::extract_return_type(&func.sig.output);

        let doc_comment = Self::extract_doc_comment(&func.attrs);

        let display_name = Self::extract_display_name(&doc_comment, &name);

        let span = self.span_to_source_span(func.sig.ident.span());

        let source_code = self.extract_source_code(func);

        let cell = CellInfo {
            id: CellId::new(0), // Assigned later by GraphEngine
            name,
            display_name,
            dependencies,
            return_type,
            doc_comment,
            source_code,
            span,
            source_file: self.source_file.clone(),
        };

        self.cells.push(cell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse(source: &str) -> ParseResult {
        let mut parser = CellParser::new();
        parser.parse_str(source, &PathBuf::from("test.rs")).unwrap()
    }

    #[test]
    fn test_parse_simple_cell() {
        let source = r#"
            use venus::prelude::*;

            #[venus::cell]
            pub fn config() -> Config {
                Config::default()
            }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        assert_eq!(result.code_cells[0].name, "config");
        assert!(result.code_cells[0].dependencies.is_empty());
        assert_eq!(result.code_cells[0].return_type, "Config");
    }

    #[test]
    fn test_parse_cell_with_dependencies() {
        let source = r#"
            #[venus::cell]
            pub fn process(config: &Config, data: &DataFrame) -> Result {
                Ok(())
            }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        assert_eq!(result.code_cells[0].name, "process");
        assert_eq!(result.code_cells[0].dependencies.len(), 2);

        assert_eq!(result.code_cells[0].dependencies[0].param_name, "config");
        assert_eq!(result.code_cells[0].dependencies[0].param_type, "Config");
        assert!(result.code_cells[0].dependencies[0].is_ref);

        assert_eq!(result.code_cells[0].dependencies[1].param_name, "data");
        assert_eq!(result.code_cells[0].dependencies[1].param_type, "DataFrame");
    }

    #[test]
    fn test_parse_doc_comments() {
        let source = r#"
            /// This is a cell
            /// with multiple lines
            /// of documentation.
            #[venus::cell]
            pub fn documented() -> i32 {
                42
            }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        assert!(result.code_cells[0].doc_comment.is_some());
        let doc = result.code_cells[0].doc_comment.as_ref().unwrap();
        assert!(doc.contains("This is a cell"));
        assert!(doc.contains("multiple lines"));
    }

    #[test]
    fn test_parse_multiple_cells() {
        let source = r#"
            #[venus::cell]
            pub fn a() -> i32 { 1 }

            fn not_a_cell() {}

            #[venus::cell]
            pub fn b(a: &i32) -> i32 { *a + 1 }

            #[venus::cell]
            pub fn c(b: &i32) -> i32 { *b + 1 }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 3);
        assert_eq!(result.code_cells[0].name, "a");
        assert_eq!(result.code_cells[1].name, "b");
        assert_eq!(result.code_cells[2].name, "c");
    }

    #[test]
    fn test_skip_non_cell_functions() {
        let source = r#"
            fn regular_function() {}

            pub fn another_regular() -> i32 { 0 }

            #[some_other_attr]
            fn with_other_attr() {}

            #[venus::cell]
            pub fn actual_cell() -> i32 { 42 }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        assert_eq!(result.code_cells[0].name, "actual_cell");
    }

    #[test]
    fn test_mutable_reference() {
        let source = r#"
            #[venus::cell]
            pub fn mutator(data: &mut Vec<i32>) -> () {
                data.push(1);
            }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        assert_eq!(result.code_cells[0].dependencies.len(), 1);
        assert!(result.code_cells[0].dependencies[0].is_ref);
        assert!(result.code_cells[0].dependencies[0].is_mut);
    }

    #[test]
    fn test_skip_ctx_parameter() {
        let source = r#"
            #[venus::cell]
            pub fn with_context(ctx: &mut CellContext, data: &DataFrame) -> Result {
                Ok(())
            }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        // ctx should be skipped
        assert_eq!(result.code_cells[0].dependencies.len(), 1);
        assert_eq!(result.code_cells[0].dependencies[0].param_name, "data");
    }

    #[test]
    fn test_cell_attribute_shorthand() {
        let source = r#"
            use venus::cell;

            #[cell]
            pub fn shorthand() -> i32 { 42 }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        assert_eq!(result.code_cells[0].name, "shorthand");
    }

    #[test]
    fn test_generic_return_type() {
        let source = r#"
            #[venus::cell]
            pub fn generic_cell() -> Result<DataFrame, Error> {
                Ok(DataFrame::new())
            }
        "#;

        let result = parse(source);
        assert_eq!(result.code_cells.len(), 1);
        assert!(result.code_cells[0].return_type.contains("Result"));
        assert!(result.code_cells[0].return_type.contains("DataFrame"));
    }

    #[test]
    fn test_markdown_cell_splitting() {
        let source = r#"
#[venus::cell]
pub fn config() -> i32 {
    42
}

// # First Markdown Cell
//
// Edit this content...


// # Second Markdown Cell
//
// Edit this content...
"#;

        let result = parse(source);

        println!("\n=== Parse Result ===");
        println!("Code cells: {}", result.code_cells.len());
        println!("Markdown cells: {}", result.markdown_cells.len());
        for (i, md) in result.markdown_cells.iter().enumerate() {
            println!("\nMarkdown cell {}:", i);
            println!("  Lines: {}-{}", md.span.start_line, md.span.end_line);
            println!("  Content: {:?}", md.content);
        }

        assert_eq!(result.code_cells.len(), 1, "Should have 1 code cell");
        assert_eq!(result.markdown_cells.len(), 2, "Should have 2 markdown cells");

        // Check first markdown cell
        assert_eq!(result.markdown_cells[0].span.start_line, 7);
        assert!(result.markdown_cells[0].content.contains("First Markdown Cell"));

        // Check second markdown cell
        assert_eq!(result.markdown_cells[1].span.start_line, 12);
        assert!(result.markdown_cells[1].content.contains("Second Markdown Cell"));
    }

    #[test]
    fn test_simple_rs_file() {
        // Parse the actual simple.rs file to see what we get
        let path = std::env::current_dir()
            .unwrap()
            .join("../../examples/simple.rs");
        if !path.exists() {
            println!("Skipping test - simple.rs not found at {:?}", path);
            return;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let result = parse(&source);

        println!("\n=== simple.rs Parse Result ===");
        println!("Code cells: {}", result.code_cells.len());
        println!("Markdown cells: {}", result.markdown_cells.len());
        for (i, md) in result.markdown_cells.iter().enumerate() {
            println!("\nMarkdown cell {}:", i);
            println!("  Lines: {}-{}", md.span.start_line, md.span.end_line);
            println!("  Content length: {}", md.content.len());
            println!("  Content preview: {:?}", &md.content.chars().take(100).collect::<String>());
        }
    }
}
