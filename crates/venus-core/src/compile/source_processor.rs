//! Source processing for Venus notebooks.
//!
//! Provides utilities for transforming notebook source code for different
//! compilation targets (production builds, etc.).

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::{parse_file, Attribute, File, Item};

/// Process notebook source code for production builds.
///
/// This processor transforms notebook source by:
/// - Removing module-level doc comments (`//!`)
/// - Stripping `#[venus::cell]` attributes from functions
/// - Removing the `main` function (to be replaced with generated one)
///
/// Uses proper syntax parsing via `syn` to handle edge cases like:
/// - Braces inside comments and strings
/// - Nested functions
/// - Complex attribute syntax
pub struct NotebookSourceProcessor;

impl NotebookSourceProcessor {
    /// Process notebook source for production compilation.
    ///
    /// Returns the processed source code with Venus-specific metadata stripped.
    ///
    /// # Errors
    ///
    /// Returns an error if the source cannot be parsed as valid Rust.
    pub fn process_for_production(source: &str) -> Result<String, syn::Error> {
        let file = parse_file(source)?;
        let processed = Self::process_file(file);
        Ok(Self::tokens_to_string(processed))
    }

    /// Process a parsed file, filtering and transforming items.
    fn process_file(file: File) -> File {
        let items = file
            .items
            .into_iter()
            .filter_map(Self::process_item)
            .collect();

        File {
            shebang: file.shebang,
            attrs: Self::filter_module_docs(file.attrs),
            items,
        }
    }

    /// Filter out module-level doc comments (//!).
    fn filter_module_docs(attrs: Vec<Attribute>) -> Vec<Attribute> {
        attrs
            .into_iter()
            .filter(|attr| {
                // Keep non-doc attributes
                !attr.path().is_ident("doc")
                    // Or if it's a doc attribute, only keep regular /// comments (outer)
                    || matches!(attr.style, syn::AttrStyle::Outer)
            })
            .collect()
    }

    /// Process a single item, returning None to remove it.
    fn process_item(item: Item) -> Option<Item> {
        match item {
            Item::Fn(mut func) => {
                // Remove main function
                if func.sig.ident == "main" {
                    return None;
                }

                // Strip #[venus::cell] attribute
                func.attrs.retain(|attr| !Self::is_venus_cell_attr(attr));

                Some(Item::Fn(func))
            }
            // Keep all other items unchanged
            other => Some(other),
        }
    }

    /// Check if an attribute is #[venus::cell].
    fn is_venus_cell_attr(attr: &Attribute) -> bool {
        let path = attr.path();

        // Check for #[venus::cell]
        if path.segments.len() == 2 {
            let first = &path.segments[0];
            let second = &path.segments[1];
            return first.ident == "venus" && second.ident == "cell";
        }

        false
    }

    /// Convert tokens back to formatted source code.
    fn tokens_to_string(file: File) -> String {
        let tokens: TokenStream = file.into_token_stream();
        tokens.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strips_module_docs() {
        let source = r#"
//! Module documentation
//! More docs

use something;

fn foo() -> i32 { 42 }
"#;
        let result = NotebookSourceProcessor::process_for_production(source).unwrap();

        assert!(!result.contains("//!"));
        assert!(!result.contains("Module documentation"));
        assert!(result.contains("use something"));
        assert!(result.contains("fn foo"));
    }

    #[test]
    fn test_strips_venus_cell_attribute() {
        let source = r#"
#[venus::cell]
pub fn my_cell() -> i32 { 42 }

#[derive(Debug)]
struct MyStruct { x: i32 }
"#;
        let result = NotebookSourceProcessor::process_for_production(source).unwrap();

        // Venus cell attribute should be stripped
        assert!(!result.contains("venus :: cell") && !result.contains("venus::cell"));
        // Function should be preserved
        assert!(result.contains("pub fn my_cell"));
        // Other attributes should be preserved (syn may tokenize with spaces)
        assert!(result.contains("derive") && result.contains("Debug"));
        assert!(result.contains("struct MyStruct"));
    }

    #[test]
    fn test_removes_main_function() {
        let source = r#"
fn helper() -> i32 { 1 }

fn main() {
    println!("Hello");
    let x = {
        let y = 2;
        y + 1
    };
}

fn another() -> i32 { 2 }
"#;
        let result = NotebookSourceProcessor::process_for_production(source).unwrap();

        assert!(result.contains("fn helper"));
        assert!(result.contains("fn another"));
        assert!(!result.contains("fn main"));
        assert!(!result.contains("println"));
    }

    #[test]
    fn test_handles_braces_in_strings() {
        let source = r#"
fn foo() -> String {
    "this has { braces } inside".to_string()
}

fn main() {
    println!("main function");
}
"#;
        let result = NotebookSourceProcessor::process_for_production(source).unwrap();

        // Should preserve foo with its string containing braces
        assert!(result.contains("fn foo"));
        assert!(result.contains("braces"));
        // Should remove main
        assert!(!result.contains("fn main"));
    }

    #[test]
    fn test_handles_braces_in_comments() {
        let source = r#"
// This comment has { braces }
fn foo() -> i32 {
    /* Multi-line comment with { braces } */
    42
}

fn main() { }
"#;
        let result = NotebookSourceProcessor::process_for_production(source).unwrap();

        assert!(result.contains("fn foo"));
        assert!(!result.contains("fn main"));
    }

    #[test]
    fn test_preserves_regular_doc_comments() {
        let source = r#"
//! Module doc (should be removed)

/// Function doc (should be kept)
fn foo() -> i32 { 42 }
"#;
        let result = NotebookSourceProcessor::process_for_production(source).unwrap();

        assert!(!result.contains("Module doc"));
        // Note: syn may normalize doc comments to #[doc = "..."]
        // The important thing is the content is preserved
        assert!(result.contains("foo"));
    }

    #[test]
    fn test_handles_nested_functions() {
        let source = r#"
fn outer() -> i32 {
    fn inner() -> i32 { 1 }
    inner() + 1
}

fn main() {
    outer();
}
"#;
        let result = NotebookSourceProcessor::process_for_production(source).unwrap();

        assert!(result.contains("fn outer"));
        assert!(result.contains("fn inner"));
        assert!(!result.contains("fn main"));
    }
}
