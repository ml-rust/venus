use std::fs;
use std::path::{Path, PathBuf};

use super::super::{NotebookContext, ResolvedManifest};
use tempfile::TempDir;

pub(super) fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

pub(super) fn package(root: &Path, path: &str, name: &str, extra: &str) {
    write(
        root,
        &format!("{path}/Cargo.toml"),
        &format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\n{extra}"),
    );
    write(
        root,
        &format!("{path}/src/lib.rs"),
        "pub fn value() -> i32 { 42 }",
    );
}

pub(super) fn workspace() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        r#"
[workspace]
members = ["member", "internal", "direct", "patched"]
[workspace.dependencies]
internal-alias = { package = "internal", path = "internal", default-features = false, features = ["base"] }
unused = "999.0"
[patch.crates-io]
patched = { path = "patched" }
"#,
    );
    package(
        dir.path(),
        "internal",
        "internal",
        "\n[features]\nbase = []\nextra = []\n",
    );
    package(dir.path(), "direct", "direct", "");
    package(dir.path(), "patched", "patched", "");
    package(
        dir.path(),
        "member",
        "member",
        r#"
[dependencies.internal-alias]
workspace = true
features = [
    "extra",
    "base",
]
[dependencies.direct]
path = "../direct"
"#,
    );
    let notebook = dir.path().join("member/notebooks/report.rs");
    write(dir.path(), "member/notebooks/report.rs", "");
    (dir, notebook)
}

pub(super) fn resolve(path: &Path, source: &str) -> ResolvedManifest {
    ResolvedManifest::resolve(&NotebookContext::for_notebook(path).unwrap(), source).unwrap()
}

#[cfg(unix)]
pub(super) fn symlinked_workspace() -> (TempDir, PathBuf) {
    let (dir, notebook) = workspace();
    let linked = dir.path().join("linked.rs");
    std::os::unix::fs::symlink(notebook, &linked).unwrap();
    (dir, linked)
}
