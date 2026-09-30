//! CLI execution with inherited workspace dependencies.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn run_notebook(notebook: &Path, current_dir: &Path, expected: &str) -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_venus"))
        .current_dir(current_dir)
        .arg("run")
        .arg(notebook)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "venus run returned {}. stdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    assert!(
        stdout.contains(expected),
        "Missing {expected}. stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("Completed"),
        "Missing completion. stdout:\n{stdout}"
    );
    Ok(())
}

#[test]
fn runtime_consumes_inherited_internal_dependency() -> std::io::Result<()> {
    let workspace = TempDir::new()?;
    let unrelated_directory = TempDir::new()?;
    let root = workspace.path();
    let notebook_directory = root.join("app/notebooks");
    let helper_directory = root.join("internal-helper/src");
    fs::create_dir_all(&notebook_directory)?;
    fs::create_dir_all(root.join("app/src"))?;
    fs::create_dir_all(&helper_directory)?;
    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["app", "internal-helper"]
resolver = "2"

[workspace.dependencies]
internal-helper = { path = "internal-helper" }
"#,
    )?;
    fs::write(
        root.join("app/Cargo.toml"),
        r#"[package]
name = "notebook-app"
version = "0.1.0"
edition = "2024"

[dependencies]
internal-helper.workspace = true
"#,
    )?;
    fs::write(root.join("app/src/lib.rs"), "")?;
    fs::write(
        root.join("internal-helper/Cargo.toml"),
        r#"[package]
name = "internal-helper"
version = "0.1.0"
edition = "2024"
"#,
    )?;
    let helper_source = helper_directory.join("lib.rs");
    fs::write(
        &helper_source,
        r#"pub fn value() -> &'static str { "workspace-first" }
"#,
    )?;
    let notebook = notebook_directory.join("runtime.rs");
    fs::write(
        &notebook,
        r#"#[venus::cell]
pub fn internal_value() -> String {
    internal_helper::value().to_string()
}
"#,
    )?;

    run_notebook(&notebook, unrelated_directory.path(), "workspace-first")?;
    fs::write(
        &helper_source,
        r#"pub fn value() -> &'static str { "workspace-updated" }
"#,
    )?;
    run_notebook(&notebook, unrelated_directory.path(), "workspace-updated")
}
