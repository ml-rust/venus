use super::super::{NotebookContext, ResolvedManifest};
use super::fixtures::*;
use crate::compile::{CompilerConfig, ManifestConfig};
use std::fs;
use std::path::Path;
use toml::{Table, Value};

#[test]
fn member_inherits_declared_aliases_and_features_from_workspace() {
    let (dir, notebook) = workspace();
    let resolved = resolve(&notebook, "");
    let alias = &resolved.dependencies["internal-alias"];
    assert_eq!(alias["package"].as_str(), Some("internal"));
    assert_eq!(alias["default-features"].as_bool(), Some(false));
    assert_eq!(
        alias["features"].as_array().unwrap(),
        &[Value::String("base".into()), Value::String("extra".into())]
    );
    assert_eq!(
        Path::new(alias["path"].as_str().unwrap()),
        dir.path().join("internal").canonicalize().unwrap()
    );
    assert_eq!(
        Path::new(resolved.dependencies["direct"]["path"].as_str().unwrap()),
        dir.path().join("direct").canonicalize().unwrap()
    );
    assert!(!resolved.dependencies.contains_key("unused"));
    assert_eq!(
        Path::new(
            resolved.patches["crates-io"]["patched"]["path"]
                .as_str()
                .unwrap()
        ),
        dir.path().join("patched").canonicalize().unwrap()
    );
    assert!(resolved.reexports().contains("pub use internal_alias;"));
    let manifest: Table = toml::from_str(
        &resolved
            .render(&ManifestConfig {
                standalone_workspace: true,
                ..ManifestConfig::default()
            })
            .unwrap(),
    )
    .unwrap();
    assert!(manifest.contains_key("workspace"));
}

#[test]
fn explicit_workspace_location_resolves_inherited_paths() {
    let (dir, notebook) = workspace();
    let member = dir.path().join("member/Cargo.toml");
    let source = fs::read_to_string(&member).unwrap().replace(
        "version = \"0.1.0\"",
        "version = \"0.1.0\"\nworkspace = \"..\"",
    );
    fs::write(member, source).unwrap();
    assert_eq!(
        resolve(&notebook, "").dependencies["internal-alias"]["package"].as_str(),
        Some("internal")
    );
}

#[test]
fn notebook_declarations_replace_parent_aliases_and_rebase_paths() {
    let (dir, notebook) = workspace();
    let resolved = resolve(
        &notebook,
        r#"
//! ```cargo
//! [dependencies.internal-alias]
//! package = "direct"
//! path = "../../direct"
//! features = []
//! ```
"#,
    );
    assert_eq!(resolved.dependencies.len(), 2);
    assert_eq!(
        resolved.dependencies["internal-alias"]["package"].as_str(),
        Some("direct")
    );
    assert_eq!(
        Path::new(
            resolved.dependencies["internal-alias"]["path"]
                .as_str()
                .unwrap()
        ),
        dir.path().join("direct").canonicalize().unwrap()
    );
}

#[test]
fn virtual_root_notebooks_retain_catalog_dependencies() {
    let (dir, _) = workspace();
    write(dir.path(), "root.rs", "");
    let resolved = resolve(&dir.path().join("root.rs"), "");
    assert!(resolved.dependencies.contains_key("unused"));
    assert!(resolved.dependencies.contains_key("internal-alias"));
}

#[test]
fn excluded_package_does_not_inherit_ancestor_patches() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "Cargo.toml",
        "[workspace]\nmembers = []\nexclude = [\"excluded\"]\n[patch.crates-io]\nremote = { git = \"https://example.invalid/remote\" }\n",
    );
    package(
        dir.path(),
        "excluded",
        "excluded",
        "[dependencies]\nserde = \"1\"\n",
    );
    write(dir.path(), "excluded/notebook.rs", "");
    let resolved = resolve(&dir.path().join("excluded/notebook.rs"), "");
    assert!(resolved.patches.is_empty());
    assert!(resolved.dependencies.contains_key("serde"));
}

#[test]
fn target_dependencies_keep_cfg_and_named_triple_gates() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "app",
        "app",
        r#"
[target.'cfg(unix)'.dependencies]
native = "1"
[target.x86_64-pc-windows-msvc.dependencies]
windows-api = "2"
"#,
    );
    let resolved = resolve(&dir.path().join("app/notebook.rs"), "");
    assert!(resolved.dependencies.is_empty());
    assert_eq!(resolved.targets.len(), 2);
    let exports = resolved.reexports();
    assert!(exports.contains("#[cfg(any(unix))]\npub use native;"));
    assert!(exports.contains("pub use windows_api;"));
    let script = resolved.build_script();
    assert!(script.contains("x86_64-pc-windows-msvc"));
    assert!(script.contains("cargo:rustc-check-cfg=cfg(venus_target_"));
}

#[test]
fn git_registry_selectors_and_multiline_tables_survive_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let resolved = resolve(
        &dir.path().join("notebook.rs"),
        r#"
//! ```cargo
//! [dependencies.renamed]
//! package = "actual"
//! git = "https://example.invalid/actual"
//! rev = "123abc"
//! default-features = false
//! features = [
//!   "a",
//!   "b",
//! ]
//! [dependencies.registry-alias]
//! package = "another"
//! version = "1"
//! registry = "private"
//! ```
"#,
    );
    assert_eq!(
        resolved.dependencies["renamed"]["rev"].as_str(),
        Some("123abc")
    );
    assert_eq!(
        resolved.dependencies["registry-alias"]["registry"].as_str(),
        Some("private")
    );
    assert_eq!(resolved.external_dependencies()[0].features.len(), 0);
    assert_eq!(
        resolved.dependencies["renamed"]["features"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn malformed_manifests_and_unresolved_inheritance_return_contextual_errors() {
    let dir = tempfile::tempdir().unwrap();
    let notebook = dir.path().join("notebook.rs");
    write(dir.path(), "Cargo.toml", "[dependencies\n");
    let context = NotebookContext::for_notebook(&notebook).unwrap();
    assert!(
        ResolvedManifest::resolve(&context, "")
            .unwrap_err()
            .to_string()
            .contains("Cargo.toml")
    );
    fs::remove_file(dir.path().join("Cargo.toml")).unwrap();
    let context = NotebookContext::for_notebook(&notebook).unwrap();
    let error = ResolvedManifest::resolve(
        &context,
        "//! ```cargo\n//! [dependencies]\n//! missing.workspace = true\n//! ```",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("missing"));
    assert!(error.contains("owning workspace"));
    assert!(
        ResolvedManifest::resolve(
            &context,
            "//! ```cargo\n//! [dependencies]\n//! alias = { features = [1] }\n//! ```"
        )
        .is_err()
    );
}

#[test]
fn canonical_manifest_changes_when_parent_features_change() {
    let (dir, notebook) = workspace();
    let before = resolve(&notebook, "").canonical();
    let path = dir.path().join("member/Cargo.toml");
    let source = fs::read_to_string(&path)
        .unwrap()
        .replace("    \"extra\",\n", "");
    fs::write(path, source).unwrap();
    let after = resolve(&notebook, "").canonical();
    assert_ne!(before, after);
}

#[test]
fn target_override_rejects_conflicting_generic_dependency_source() {
    let (dir, notebook) = workspace();
    let error = ResolvedManifest::resolve(
        &NotebookContext::for_notebook(&notebook).unwrap(),
        r#"
//! ```cargo
//! [target.'cfg(unix)'.dependencies]
//! internal-alias = { package = "direct", path = "../../direct" }
//! ```
"#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("internal-alias"));
    assert!(error.contains("different packages or sources"));
    assert!(dir.path().exists());
}

#[test]
fn rendered_manifests_preserve_native_path_strings() {
    for path in [
        r"C:\notebooks\local",
        r"\\?\C:\notebooks\local",
        r"\\server\share\local",
        r"\\?\UNC\server\share\local",
        r"local\component",
        r#"/notebooks/quoted "component"/local"#,
    ] {
        let dependency = Value::Table(Table::from_iter([(
            "path".into(),
            Value::String(path.into()),
        )]));
        let resolved = ResolvedManifest {
            dependencies: Table::from_iter([("local".into(), dependency.clone())]),
            targets: Table::from_iter([(
                "cfg(windows)".into(),
                Value::Table(Table::from_iter([(
                    "dependencies".into(),
                    Value::Table(Table::from_iter([("local".into(), dependency.clone())])),
                )])),
            )]),
            patches: Table::from_iter([(
                "crates-io".into(),
                Value::Table(Table::from_iter([("local".into(), dependency)])),
            )]),
            ..ResolvedManifest::default()
        };
        let manifest: Table =
            toml::from_str(&resolved.render(&ManifestConfig::default()).unwrap()).unwrap();
        for dependency in [
            &manifest["dependencies"]["local"],
            &manifest["target"]["cfg(windows)"]["dependencies"]["local"],
            &manifest["patch"]["crates-io"]["local"],
        ] {
            assert_eq!(dependency["path"].as_str(), Some(path));
        }
    }
}

#[test]
fn generated_paths_match_canonical_sources_and_runtime_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let component = if cfg!(unix) {
        "native \\ \"quoted\" paths"
    } else {
        "native ' paths"
    };
    let root = dir.path().join(component);
    package(&root, "local", "local", "");
    package(&root, "patched", "patched", "");
    package(&root, "runtime", "venus", "");
    package(
        &root,
        "app",
        "app",
        r#"
[dependencies]
local = { path = "../local/./" }
venus = { path = "../runtime/./" }
[target.'cfg(windows)'.dependencies]
local = { path = "../local" }
venus = { path = "../runtime" }
[patch.crates-io]
patched = { path = "../patched" }
"#,
    );
    write(&root, "app/notebook.rs", "");
    let notebook = root.join("app/notebook.rs");
    let config = CompilerConfig {
        venus_crate_path: Some(root.join("runtime")),
        ..CompilerConfig::default()
    };
    let resolved = resolve(&notebook, "")
        .with_runtime(&config, &notebook)
        .unwrap();
    let manifest: Table =
        toml::from_str(&resolved.render(&ManifestConfig::default()).unwrap()).unwrap();
    for (dependency, directory) in [
        (&manifest["dependencies"]["local"], "local"),
        (
            &manifest["target"]["cfg(windows)"]["dependencies"]["local"],
            "local",
        ),
        (&manifest["dependencies"]["venus"], "runtime"),
        (
            &manifest["target"]["cfg(windows)"]["dependencies"]["venus"],
            "runtime",
        ),
        (&manifest["patch"]["crates-io"]["patched"], "patched"),
    ] {
        let expected = root.join(directory).canonicalize().unwrap();
        assert_eq!(
            dependency["path"].as_str(),
            Some(expected.to_string_lossy().as_ref())
        );
        assert_eq!(Path::new(dependency["path"].as_str().unwrap()), expected);
    }
}

#[cfg(unix)]
#[test]
fn symlinked_notebook_resolves_declarations_from_its_canonical_directory() {
    let (dir, notebook) = symlinked_workspace();
    let context = NotebookContext::for_notebook(&notebook).unwrap();
    assert_eq!(
        context.directory,
        dir.path().join("member/notebooks").canonicalize().unwrap()
    );
    assert_eq!(
        context.manifest,
        Some(dir.path().join("member/Cargo.toml").canonicalize().unwrap())
    );
    let resolved = resolve(
        &notebook,
        "//! ```cargo\n//! [dependencies]\n//! internal-alias = { package = \"direct\", path = \"../../direct\" }\n//! ```",
    );
    let expected = dir.path().join("direct").canonicalize().unwrap();
    assert_eq!(
        Path::new(
            resolved.dependencies["internal-alias"]["path"]
                .as_str()
                .unwrap()
        ),
        expected
    );
}
