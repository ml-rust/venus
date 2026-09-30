use super::NotebookSession;
use crate::undo::UndoManager;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool};
use tempfile::TempDir;
use venus_core::compile::{CompilerConfig, ToolchainManager};
use venus_core::execute::ProcessExecutor;
use venus_core::graph::GraphEngine;
use venus_core::paths::NotebookDirs;

use crate::protocol::ServerMessage;
use tokio::sync::broadcast;

#[test]
fn test_session_creation() {
    // This would require a real notebook file, so we just test the types compile
    let (tx, _rx) = broadcast::channel::<ServerMessage>(16);
    drop(tx);
}

#[test]
fn notebook_state_refreshes_nearest_manifest_paths() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let root = directory.path().canonicalize()?;
    let member = root.join("member");
    std::fs::create_dir_all(member.join("notebooks"))?;
    let notebook = member.join("notebooks/runtime.rs");
    std::fs::write(&notebook, "")?;
    let root_manifest = root.join("Cargo.toml");
    std::fs::write(&root_manifest, "[workspace]\n")?;
    let session = unloaded_session(&notebook)?;
    let check_paths = |manifest: &Path, directory: &Path| {
        let ServerMessage::NotebookState {
            cargo_toml_path,
            workspace_root,
            ..
        } = session.get_state()
        else {
            panic!("Expected NotebookState");
        };
        assert_eq!(cargo_toml_path, Some(manifest.display().to_string()));
        assert_eq!(workspace_root, Some(directory.display().to_string()));
    };

    check_paths(&root_manifest, &root);
    let member_manifest = member.join("Cargo.toml");
    std::fs::write(&member_manifest, "[package]\nname = \"member\"\n")?;
    check_paths(&member_manifest, &member);
    std::fs::remove_file(member_manifest)?;
    check_paths(&root_manifest, &root);
    Ok(())
}

fn unloaded_session(notebook: &Path) -> Result<NotebookSession, Box<dyn std::error::Error>> {
    let dirs = NotebookDirs::from_notebook_path(notebook)?;
    let (tx, _rx) = broadcast::channel(16);
    Ok(NotebookSession {
        path: notebook.to_path_buf(),
        cells: Vec::new(),
        markdown_cells: Vec::new(),
        definition_cells: Vec::new(),
        graph: GraphEngine::new(),
        cell_states: HashMap::new(),
        toolchain: ToolchainManager::new()?,
        config: CompilerConfig::for_notebook(&dirs),
        universe_path: None,
        deps_hash: 0,
        tx,
        executing: false,
        cell_outputs: HashMap::new(),
        executor: ProcessExecutor::new(&dirs.state_dir)?,
        execution_timeout: None,
        interrupted: Arc::new(AtomicBool::new(false)),
        widget_values: HashMap::new(),
        widget_defs: HashMap::new(),
        cell_output_history: HashMap::new(),
        cell_history_index: HashMap::new(),
        undo_manager: UndoManager::new(),
        pending_edits: HashMap::new(),
    })
}

#[test]
fn reload_refreshes_generated_dependency_context() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let root = directory.path().canonicalize()?;
    let member = root.join("member");
    std::fs::create_dir_all(member.join("notebooks"))?;
    std::fs::create_dir_all(member.join("src"))?;
    std::fs::create_dir_all(root.join("internal-helper/src"))?;
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["internal-helper"]
exclude = ["member"]
resolver = "2"

[workspace.dependencies]
root_alias = { package = "internal-helper", path = "internal-helper" }
"#,
    )?;
    std::fs::write(
        root.join("internal-helper/Cargo.toml"),
        r#"[package]
name = "internal-helper"
version = "0.1.0"
edition = "2024"
"#,
    )?;
    std::fs::write(
        root.join("internal-helper/src/lib.rs"),
        "pub fn value() -> i32 { 42 }\n",
    )?;
    std::fs::write(member.join("src/lib.rs"), "")?;
    let notebook = member.join("notebooks/runtime.rs");
    std::fs::write(&notebook, "#[venus::cell]\npub fn value() -> i32 { 42 }\n")?;
    let mut session = unloaded_session(&notebook)?;
    let dirs = NotebookDirs::from_notebook_path(&notebook)?;
    let universe_manifest = dirs.build_dir.join("universe/Cargo.toml");

    session.reload()?;
    check_dependency_alias(&universe_manifest, "root_alias", "member_alias")?;
    let member_manifest = member.join("Cargo.toml");
    std::fs::write(
        &member_manifest,
        r#"[package]
name = "notebook-member"
version = "0.1.0"
edition = "2024"

[workspace]

[dependencies]
member_alias = { package = "internal-helper", path = "../internal-helper" }
"#,
    )?;
    session.reload()?;
    check_dependency_alias(&universe_manifest, "member_alias", "root_alias")?;
    std::fs::remove_file(member_manifest)?;
    session.reload()?;
    check_dependency_alias(&universe_manifest, "root_alias", "member_alias")
}

fn check_dependency_alias(
    manifest: &Path,
    present: &str,
    absent: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(manifest)?;
    let declared = |alias: &str| {
        let dependency_table = format!("[dependencies.{alias}]");
        source.lines().any(|line| {
            line.trim() == dependency_table
                || line
                    .split_once('=')
                    .is_some_and(|(name, _)| name.trim() == alias)
        })
    };
    assert!(
        declared(present),
        "Missing {present} in generated manifest:\n{source}"
    );
    assert!(
        !declared(absent),
        "Unexpected {absent} in generated manifest:\n{source}"
    );
    Ok(())
}
