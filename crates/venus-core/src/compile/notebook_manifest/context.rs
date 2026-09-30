use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use toml::{Table, Value};

use crate::error::{Error, Result};

/// Find the nearest Cargo manifest above a notebook.
pub fn find_notebook_manifest(notebook_path: &Path) -> Option<PathBuf> {
    let path = absolute(notebook_path).ok()?;
    path.parent()?
        .ancestors()
        .map(|dir| dir.join("Cargo.toml"))
        .find(|path| path.is_file())
}

#[derive(Debug, Clone)]
pub(in crate::compile) struct NotebookContext {
    pub directory: PathBuf,
    pub manifest: Option<PathBuf>,
}

impl NotebookContext {
    pub fn for_notebook(path: &Path) -> Result<Self> {
        let path = absolute(path)?;
        let directory = path
            .parent()
            .ok_or_else(|| manifest_error(&path, "Notebook has no parent directory"))?
            .to_path_buf();
        Ok(Self {
            directory,
            manifest: find_notebook_manifest(&path),
        })
    }

    pub fn from_manifest(manifest: Option<PathBuf>) -> Self {
        let directory = manifest
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            directory,
            manifest,
        }
    }

    pub(super) fn read(&self) -> Result<Option<ParentManifest>> {
        let Some(path) = &self.manifest else {
            return Ok(None);
        };
        let path = absolute(path)?;
        let document = read_document(&path)?;
        let workspace_path = owning_workspace(&path, &document)?;
        let workspace = match workspace_path {
            Some(root) if root == path => Some((root, document.clone())),
            Some(root) => {
                let document = read_document(&root)?;
                Some((root, document))
            }
            None => None,
        };
        Ok(Some(ParentManifest {
            path,
            document,
            workspace,
        }))
    }
}

pub(super) struct ParentManifest {
    pub path: PathBuf,
    pub document: Table,
    pub workspace: Option<(PathBuf, Table)>,
}

fn owning_workspace(path: &Path, document: &Table) -> Result<Option<PathBuf>> {
    if document.contains_key("workspace") {
        return Ok(Some(path.to_path_buf()));
    }
    if let Some(location) = document
        .get("package")
        .and_then(Value::as_table)
        .and_then(|package| package.get("workspace"))
    {
        let location = location
            .as_str()
            .ok_or_else(|| manifest_error(path, "package.workspace must be a directory path"))?;
        let root = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(location)
            .join("Cargo.toml");
        let root = absolute(&root)?;
        if !read_document(&root)?.contains_key("workspace") {
            return Err(manifest_error(
                &root,
                "package.workspace does not identify a workspace root",
            ));
        }
        return Ok(Some(root));
    }
    for ancestor in path
        .parent()
        .and_then(Path::parent)
        .into_iter()
        .flat_map(Path::ancestors)
    {
        let root = ancestor.join("Cargo.toml");
        if root.is_file() && read_document(&root)?.contains_key("workspace") {
            let output = Command::new("cargo")
                .args([
                    "locate-project",
                    "--workspace",
                    "--message-format",
                    "plain",
                    "--manifest-path",
                ])
                .arg(path)
                .output()
                .map_err(|error| {
                    manifest_error(
                        path,
                        &format!(
                            "Cannot discover owning workspace: {error}. Check Cargo installation"
                        ),
                    )
                })?;
            if !output.status.success() {
                return Err(manifest_error(
                    path,
                    &format!(
                        "Cannot discover owning workspace: {}. Correct the manifest or workspace membership",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ),
                ));
            }
            let workspace = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
            return if workspace == path {
                Ok(None)
            } else {
                Ok(Some(absolute(&workspace)?))
            };
        }
    }
    Ok(None)
}

pub(super) fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(path.canonicalize().unwrap_or(path))
}

pub(super) fn read_document(path: &Path) -> Result<Table> {
    let source = fs::read_to_string(path)
        .map_err(|error| manifest_error(path, &format!("Cannot read manifest: {error}")))?;
    let document: Table = toml::from_str(&source)
        .map_err(|error| manifest_error(path, &format!("Cannot parse manifest: {error}")))?;
    for section in [
        "package",
        "workspace",
        "dependencies",
        "target",
        "features",
        "patch",
    ] {
        if document.get(section).is_some_and(|value| !value.is_table()) {
            return Err(manifest_error(path, &format!("{section} must be a table")));
        }
    }
    Ok(document)
}

pub(super) fn manifest_error(path: &Path, message: &str) -> Error {
    Error::Compilation {
        cell_id: None,
        message: format!(
            "Cargo context '{}': {message}. Correct the manifest dependency declarations",
            path.display()
        ),
    }
}
