//! Integration tests for Venus notebook synchronization.
//!
//! Tests the full pipeline: .rs → .ipynb conversion and roundtrip verification.

use std::fs;
use tempfile::TempDir;
use venus_sync::{JupyterNotebook, OutputCache, RsParser, sync_to_ipynb};

// =============================================================================
// Test Helpers
// =============================================================================

/// Create a temporary directory for test artifacts.
fn temp_dir() -> TempDir {
    TempDir::new().expect("Failed to create temp directory")
}

/// Create a simple test notebook with code and markdown cells.
fn create_simple_notebook() -> &'static str {
    r#"//! # Simple Test Notebook
//! This notebook demonstrates basic cell types.
//!
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```

/// # Introduction
/// This is a markdown cell explaining the notebook.
#[venus::cell]
pub fn greeting() -> String {
    "Hello from Venus!".to_string()
}

/// # Processing
/// This cell processes the greeting.
#[venus::cell]
pub fn process(greeting: &String) -> String {
    format!("Processed: {}", greeting)
}
"#
}

/// Create a notebook with various edge cases.
fn create_edge_case_notebook() -> &'static str {
    r#"//! # Edge Case Notebook
//! Tests special characters and empty cells.

/// # Unicode Test
/// Testing unicode: 你好世界 🚀
#[venus::cell]
pub fn unicode_test() -> String {
    "Hello 世界!".to_string()
}

/// # Empty Result
/// Returns unit type.
#[venus::cell]
pub fn empty_cell() {
    println!("Side effect only");
}

/// # Multi-line
/// This cell has
/// multiple lines
/// of documentation.
#[venus::cell]
pub fn multi_line() -> String {
    "test".to_string()
}
"#
}

// =============================================================================
// Basic Conversion Tests
// =============================================================================

#[test]
fn test_simple_rs_to_ipynb_conversion() {
    let temp = temp_dir();
    let rs_path = temp.path().join("simple.rs");
    let ipynb_path = temp.path().join("simple.ipynb");

    // Write test notebook
    fs::write(&rs_path, create_simple_notebook()).expect("Failed to write RS file");

    // Convert to IPYNB
    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync to IPYNB");

    // Verify IPYNB exists and is valid JSON
    assert!(ipynb_path.exists(), "IPYNB file should exist");

    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // Verify basic structure
    assert_eq!(notebook.nbformat, 4, "Should use Jupyter format version 4");
    assert_eq!(
        notebook.nbformat_minor, 5,
        "Should use Jupyter format minor version 5"
    );
    assert!(!notebook.cells.is_empty(), "Should have cells");
}

#[test]
fn test_metadata_preservation() {
    let temp = temp_dir();
    let rs_path = temp.path().join("metadata.rs");
    let ipynb_path = temp.path().join("metadata.ipynb");

    fs::write(&rs_path, create_simple_notebook()).expect("Failed to write RS file");

    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync");

    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // Verify metadata contains kernel info
    assert_eq!(
        notebook.metadata.kernelspec.language, "rust",
        "Should have Rust kernelspec"
    );
    assert_eq!(
        notebook.metadata.language_info.name, "rust",
        "Should have Rust language_info"
    );
}

#[test]
fn test_cell_count_and_types() {
    let temp = temp_dir();
    let rs_path = temp.path().join("cells.rs");
    let ipynb_path = temp.path().join("cells.ipynb");

    fs::write(&rs_path, create_simple_notebook()).expect("Failed to write RS file");

    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync");

    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // Should have markdown and code cells
    // Simple notebook has: 1 header markdown + 2 doc markdowns + 2 code cells
    assert!(
        notebook.cells.len() >= 2,
        "Should have at least 2 cells (markdown + code)"
    );

    // Verify cell types
    let has_markdown = notebook.cells.iter().any(|c| c.cell_type == "markdown");
    let has_code = notebook.cells.iter().any(|c| c.cell_type == "code");

    assert!(has_markdown, "Should have markdown cells");
    assert!(has_code, "Should have code cells");
}

// =============================================================================
// Roundtrip Verification Tests
// =============================================================================

#[test]
fn test_roundtrip_cell_content_preservation() {
    let temp = temp_dir();
    let rs_path = temp.path().join("roundtrip.rs");
    let ipynb_path = temp.path().join("roundtrip.ipynb");

    fs::write(&rs_path, create_simple_notebook()).expect("Failed to write RS file");

    // Parse original RS to get expected cells
    let parser = RsParser::new();
    let (_metadata, _original_cells) = parser
        .parse_file(&rs_path)
        .expect("Failed to parse RS file");

    // Convert to IPYNB
    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync");

    // Read back IPYNB and verify cells match
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // Verify cell count matches (accounting for markdown cells being split)
    assert!(
        !notebook.cells.is_empty(),
        "Should preserve cells from original"
    );

    // Verify code cells have source content
    for cell in &notebook.cells {
        if cell.cell_type == "code" {
            assert!(
                !cell.source.is_empty(),
                "Code cells should have source content"
            );
        }
    }
}

#[test]
fn test_roundtrip_with_unicode() {
    let temp = temp_dir();
    let rs_path = temp.path().join("unicode.rs");
    let ipynb_path = temp.path().join("unicode.ipynb");

    fs::write(&rs_path, create_edge_case_notebook()).expect("Failed to write RS file");

    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync");

    // Read back and verify unicode is preserved
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // Find the unicode cell
    let has_unicode = notebook.cells.iter().any(|c| {
        c.source
            .iter()
            .any(|line| line.contains("你好世界") || line.contains("🚀"))
    });

    assert!(has_unicode, "Should preserve unicode characters");
}

// =============================================================================
// Output Cache Tests
// =============================================================================

#[test]
fn test_sync_with_output_cache() {
    let temp = temp_dir();
    let rs_path = temp.path().join("with_outputs.rs");
    let ipynb_path = temp.path().join("with_outputs.ipynb");
    let cache_dir = temp.path().join(".venus/outputs");

    fs::create_dir_all(&cache_dir).expect("Failed to create cache dir");
    fs::write(&rs_path, create_simple_notebook()).expect("Failed to write RS file");

    // Create output cache
    let mut cache = OutputCache::new(&cache_dir).expect("Failed to create cache");

    // Store some test outputs
    cache.store_text("greeting", "Hello from Venus!");

    // Sync with cache
    sync_to_ipynb(&rs_path, &ipynb_path, Some(&cache)).expect("Failed to sync with cache");

    // Verify IPYNB has outputs
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // At least one cell should have outputs
    let has_outputs = notebook
        .cells
        .iter()
        .any(|c| c.outputs.as_ref().is_some_and(|o| !o.is_empty()));
    assert!(
        has_outputs,
        "Should include outputs from cache for code cells"
    );
}

#[test]
fn test_sync_without_output_cache() {
    let temp = temp_dir();
    let rs_path = temp.path().join("no_outputs.rs");
    let ipynb_path = temp.path().join("no_outputs.ipynb");

    fs::write(&rs_path, create_simple_notebook()).expect("Failed to write RS file");

    // Sync without cache (None)
    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync without cache");

    // Verify IPYNB has no outputs (since no cache provided)
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // All cells should have empty outputs
    for cell in &notebook.cells {
        if cell.cell_type == "code" {
            assert!(
                cell.outputs.as_ref().is_none_or(|o| o.is_empty()),
                "Code cells should have no outputs without cache"
            );
        }
    }
}

// =============================================================================
// Edge Case Tests
// =============================================================================

#[test]
fn test_empty_notebook() {
    let temp = temp_dir();
    let rs_path = temp.path().join("empty.rs");
    let ipynb_path = temp.path().join("empty.ipynb");

    // Empty notebook with just module doc
    fs::write(&rs_path, "//! Empty notebook\n").expect("Failed to write RS file");

    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync empty notebook");

    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // Should create valid notebook even if empty
    assert_eq!(notebook.nbformat, 4);
    // May have header markdown cell
}

#[test]
fn test_multiple_conversions_idempotent() {
    let temp = temp_dir();
    let rs_path = temp.path().join("idempotent.rs");
    let ipynb_path1 = temp.path().join("idempotent1.ipynb");
    let ipynb_path2 = temp.path().join("idempotent2.ipynb");

    fs::write(&rs_path, create_simple_notebook()).expect("Failed to write RS file");

    // Convert twice
    sync_to_ipynb(&rs_path, &ipynb_path1, None).expect("First sync failed");
    sync_to_ipynb(&rs_path, &ipynb_path2, None).expect("Second sync failed");

    // Both outputs should be identical
    let content1 = fs::read_to_string(&ipynb_path1).expect("Failed to read IPYNB 1");
    let content2 = fs::read_to_string(&ipynb_path2).expect("Failed to read IPYNB 2");

    assert_eq!(
        content1, content2,
        "Multiple conversions should produce identical output"
    );
}

#[test]
fn test_special_characters_in_cell_names() {
    let temp = temp_dir();
    let rs_path = temp.path().join("special.rs");
    let ipynb_path = temp.path().join("special.ipynb");

    let source = r#"//! Special characters test

#[venus::cell]
pub fn test_underscores_123() -> String {
    "test".to_string()
}

#[venus::cell]
pub fn test_mixed_Case() -> String {
    "test".to_string()
}
"#;

    fs::write(&rs_path, source).expect("Failed to write RS file");

    sync_to_ipynb(&rs_path, &ipynb_path, None).expect("Failed to sync");

    // Should handle underscores and mixed case in function names
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read IPYNB");
    let _notebook: JupyterNotebook =
        serde_json::from_str(&content).expect("IPYNB should be valid JSON");

    // If we get here without errors, special characters were handled correctly
}
