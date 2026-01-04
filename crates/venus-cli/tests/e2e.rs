//! End-to-end tests for Venus CLI commands.
//!
//! These tests verify that the CLI produces expected output
//! when run against real notebook files.

#![allow(deprecated)] // Allow deprecated Command::cargo_bin for tests

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

// =============================================================================
// Test Helpers
// =============================================================================

/// Create a temporary directory with a test notebook.
struct TestNotebook {
    _temp_dir: TempDir,
    notebook_path: PathBuf,
}

impl TestNotebook {
    fn new(filename: &str, source: &str) -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let notebook_path = temp_dir.path().join(filename);
        fs::write(&notebook_path, source).expect("Failed to write notebook");

        Self {
            _temp_dir: temp_dir,
            notebook_path,
        }
    }

    fn path(&self) -> &PathBuf {
        &self.notebook_path
    }

    fn ipynb_path(&self) -> PathBuf {
        self.notebook_path.with_extension("ipynb")
    }
}

/// Create a simple notebook with primitive types.
fn simple_notebook() -> String {
    r#"//! Simple Test Notebook
//!
//! ```cargo
//! [dependencies]
//! ```

/// Returns a base number.
#[venus::cell]
pub fn base() -> i32 {
    42
}

/// Doubles the base value.
#[venus::cell]
pub fn doubled(base: &i32) -> i32 {
    base * 2
}

/// Adds ten to doubled.
#[venus::cell]
pub fn plus_ten(doubled: &i32) -> i32 {
    doubled + 10
}
"#
    .to_string()
}

/// Create a notebook with String types.
fn string_notebook() -> String {
    r#"//! String Test Notebook
//!
//! ```cargo
//! [dependencies]
//! ```

/// Returns a greeting.
#[venus::cell]
pub fn greeting() -> String {
    "Hello, Venus!".to_string()
}

/// Transforms the greeting.
#[venus::cell]
pub fn shouted(greeting: &String) -> String {
    greeting.to_uppercase()
}
"#
    .to_string()
}

// =============================================================================
// venus run Tests
// =============================================================================

#[test]
fn test_run_nonexistent_notebook() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["run", "/nonexistent/notebook.rs"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found").or(predicate::str::contains("Notebook")));
}

#[test]
fn test_run_simple_notebook() {
    let notebook = TestNotebook::new("simple.rs", &simple_notebook());

    let output = Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["run", notebook.path().to_str().unwrap()])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Print output for debugging if test fails
    if !output.status.success() {
        eprintln!("STDOUT:\n{}", stdout);
        eprintln!("STDERR:\n{}", stderr);
    }

    // Should complete successfully
    assert!(
        output.status.success(),
        "venus run should succeed. stderr: {}",
        stderr
    );

    // Should parse 3 cells
    assert!(
        stdout.contains("3 cells"),
        "Should report 3 cells. stdout: {}",
        stdout
    );

    // Should show completion message
    assert!(
        stdout.contains("Completed"),
        "Should show completion. stdout: {}",
        stdout
    );
}

#[test]
fn test_run_specific_cell() {
    let notebook = TestNotebook::new("specific.rs", &simple_notebook());

    let output = Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args([
            "run",
            notebook.path().to_str().unwrap(),
            "--cell",
            "doubled",
        ])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!("STDOUT:\n{}", stdout);
        eprintln!("STDERR:\n{}", stderr);
    }

    assert!(
        output.status.success(),
        "venus run --cell should succeed. stderr: {}",
        stderr
    );

    // Should complete with 2 cells (base and doubled)
    assert!(
        stdout.contains("Completed") && stdout.contains("2 cells"),
        "Should run 2 cells (base and doubled). stdout: {}",
        stdout
    );
}

#[test]
fn test_run_nonexistent_cell() {
    let notebook = TestNotebook::new("nonexistent_cell.rs", &simple_notebook());

    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args([
            "run",
            notebook.path().to_str().unwrap(),
            "--cell",
            "nonexistent",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn test_run_empty_notebook() {
    let empty_source = r#"//! Empty notebook
//!
//! ```cargo
//! [dependencies]
//! ```

// No cells defined
fn helper() -> i32 { 42 }
"#;

    let notebook = TestNotebook::new("empty.rs", empty_source);

    let output = Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["run", notebook.path().to_str().unwrap()])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should succeed but report no cells
    assert!(output.status.success(), "Should succeed even with no cells");
    assert!(
        stdout.contains("No cells found"),
        "Should report no cells. stdout: {}",
        stdout
    );
}

// =============================================================================
// venus sync Tests
// =============================================================================

#[test]
fn test_sync_nonexistent_notebook() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["sync", "/nonexistent/notebook.rs"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found").or(predicate::str::contains("Notebook")));
}

#[test]
fn test_sync_creates_ipynb() {
    let notebook = TestNotebook::new("sync_test.rs", &simple_notebook());

    let output = Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["sync", notebook.path().to_str().unwrap()])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!("STDOUT:\n{}", stdout);
        eprintln!("STDERR:\n{}", stderr);
    }

    assert!(
        output.status.success(),
        "venus sync should succeed. stderr: {}",
        stderr
    );

    // Check that ipynb file was created
    let ipynb_path = notebook.ipynb_path();
    assert!(
        ipynb_path.exists(),
        "Should create .ipynb file at {:?}",
        ipynb_path
    );
}

#[test]
fn test_sync_generates_valid_json() {
    let notebook = TestNotebook::new("valid_json.rs", &simple_notebook());

    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["sync", notebook.path().to_str().unwrap()])
        .assert()
        .success();

    // Read and parse the ipynb file
    let ipynb_path = notebook.ipynb_path();
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read ipynb file");

    let notebook_json: serde_json::Value =
        serde_json::from_str(&content).expect("ipynb should be valid JSON");

    // Verify basic Jupyter notebook structure
    assert!(
        notebook_json.get("cells").is_some(),
        "Should have 'cells' field"
    );
    assert!(
        notebook_json.get("metadata").is_some(),
        "Should have 'metadata' field"
    );
    assert!(
        notebook_json.get("nbformat").is_some(),
        "Should have 'nbformat' field"
    );

    // Verify cells array
    let cells = notebook_json["cells"]
        .as_array()
        .expect("cells should be array");
    assert!(!cells.is_empty(), "Should have at least one cell");
}

#[test]
fn test_sync_includes_cells() {
    let notebook = TestNotebook::new("with_cells.rs", &simple_notebook());

    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["sync", notebook.path().to_str().unwrap()])
        .assert()
        .success();

    let ipynb_path = notebook.ipynb_path();
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read ipynb file");
    let notebook_json: serde_json::Value =
        serde_json::from_str(&content).expect("ipynb should be valid JSON");

    let cells = notebook_json["cells"]
        .as_array()
        .expect("cells should be array");

    // Find code cells (should have our function definitions)
    let code_cells: Vec<_> = cells.iter().filter(|c| c["cell_type"] == "code").collect();

    // Should have at least 3 code cells (base, doubled, plus_ten)
    assert!(
        code_cells.len() >= 3,
        "Should have at least 3 code cells, found {}",
        code_cells.len()
    );
}

#[test]
fn test_sync_string_notebook() {
    let notebook = TestNotebook::new("strings.rs", &string_notebook());

    let output = Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["sync", notebook.path().to_str().unwrap()])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success(), "venus sync should succeed");

    let ipynb_path = notebook.ipynb_path();
    assert!(ipynb_path.exists(), "Should create .ipynb file");

    // Verify valid JSON
    let content = fs::read_to_string(&ipynb_path).expect("Failed to read ipynb file");
    let _: serde_json::Value = serde_json::from_str(&content).expect("Should be valid JSON");
}

// =============================================================================
// venus new Tests
// =============================================================================

#[test]
fn test_new_creates_notebook() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .current_dir(temp_dir.path())
        .args(["new", "my_notebook"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    let notebook_path = temp_dir.path().join("my_notebook.rs");
    assert!(notebook_path.exists(), "Should create notebook file");

    let content = fs::read_to_string(&notebook_path).expect("Failed to read notebook");
    assert!(
        content.contains("#[venus::cell]"),
        "Should contain cell macro"
    );
    assert!(
        content.contains("venus::prelude"),
        "Should import venus prelude"
    );
}

#[test]
fn test_new_refuses_existing() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let existing = temp_dir.path().join("existing.rs");
    fs::write(&existing, "// existing file").expect("Failed to create file");

    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .current_dir(temp_dir.path())
        .args(["new", "existing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

// =============================================================================
// General CLI Tests
// =============================================================================

#[test]
fn test_help() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Reactive notebook environment"));
}

#[test]
fn test_version() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("venus"));
}

#[test]
fn test_run_help() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("notebook"));
}

#[test]
fn test_sync_help() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ipynb").or(predicate::str::contains("notebook")));
}

// ============================================================================
// Export Command Tests
// ============================================================================

#[test]
fn test_export_creates_html() {
    let notebook = TestNotebook::new("export_test.rs", &simple_notebook());

    let output_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let html_path = output_dir.path().join("output.html");

    let output = Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args([
            "export",
            notebook.path().to_str().unwrap(),
            "-o",
            html_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprintln!("STDOUT:\n{}", stdout);
        eprintln!("STDERR:\n{}", stderr);
    }

    assert!(
        output.status.success(),
        "venus export should succeed. stderr: {}",
        stderr
    );

    // Check HTML file was created
    assert!(html_path.exists(), "HTML file should be created");

    // Check HTML content
    let html_content = std::fs::read_to_string(&html_path).expect("Failed to read HTML file");
    assert!(
        html_content.contains("<!DOCTYPE html>"),
        "Should be valid HTML"
    );
    assert!(
        html_content.contains("Venus Notebook"),
        "Should have Venus title"
    );
    assert!(html_content.contains("base"), "Should include base cell");
    assert!(
        html_content.contains("doubled"),
        "Should include doubled cell"
    );
}

#[test]
fn test_export_includes_outputs() {
    let notebook = TestNotebook::new("export_outputs.rs", &simple_notebook());

    let output_dir = tempfile::TempDir::new().expect("Failed to create temp dir");
    let html_path = output_dir.path().join("output.html");

    let output = Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args([
            "export",
            notebook.path().to_str().unwrap(),
            "-o",
            html_path.to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success(), "venus export should succeed");

    let html_content = std::fs::read_to_string(&html_path).expect("Failed to read HTML file");

    // Should have output sections
    assert!(
        html_content.contains("cell-output"),
        "Should have output sections"
    );

    // Should have success styling
    assert!(
        html_content.contains("class=\"cell success\""),
        "Should mark successful cells"
    );
}

#[test]
fn test_export_help() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["export", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HTML"));
}

// ============================================================================
// Watch Command Tests
// ============================================================================

#[test]
fn test_watch_help() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["watch", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auto-run"));
}

#[test]
fn test_watch_nonexistent_notebook() {
    Command::cargo_bin("venus")
        .expect("Failed to find venus binary")
        .args(["watch", "nonexistent.rs"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}
