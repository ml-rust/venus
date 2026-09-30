use super::fixtures::*;
use crate::compile::CompilerConfig;
use std::fs;
use toml::Value;

#[test]
fn optional_exports_follow_activation_and_forwarding_features() {
    let dir = tempfile::tempdir().unwrap();
    let resolved = resolve(
        &dir.path().join("notebook.rs"),
        r#"
//! ```cargo
//! [dependencies]
//! hidden = { version = "1", optional = true }
//! implicit = { version = "1", optional = true }
//! forwarded = { version = "1", optional = true }
//! [features]
//! default = ["outer"]
//! outer = ["activate"]
//! activate = ["dep:hidden"]
//! weak = ["hidden?/enabled"]
//! forward = ["forwarded/enabled"]
//! ```
"#,
    );
    let exports = resolved.reexports();
    let gate = exports
        .lines()
        .zip(exports.lines().skip(1))
        .find(|(_, line)| *line == "pub use hidden;")
        .unwrap()
        .0;
    assert!(gate.contains("feature = \"activate\""));
    assert!(gate.contains("feature = \"outer\""));
    assert!(gate.contains("feature = \"default\""));
    assert!(!gate.contains("feature = \"hidden\""));
    assert!(!gate.contains("feature = \"weak\""));
    assert!(exports.contains("feature = \"implicit\""));
    assert!(exports.contains("feature = \"forward\""));
}

#[test]
fn reserved_dependencies_keep_compatible_features_and_reject_conflicting_packages() {
    let dir = tempfile::tempdir().unwrap();
    let config = CompilerConfig {
        venus_crate_path: None,
        ..CompilerConfig::default()
    };
    let resolved = resolve(
        &dir.path().join("notebook.rs"),
        r#"
//! ```cargo
//! [dependencies]
//! rkyv = { version = "0.8.12", features = ["alloc"], default-features = false, optional = true }
//! serde_json = { version = "1.0.100", features = ["raw_value"], optional = true }
//! [features]
//! default = ["json"]
//! json = ["dep:serde_json", "serde_json?/raw_value"]
//! archive = ["rkyv"]
//! ```
"#,
    )
    .with_runtime(&config, dir.path())
    .unwrap();
    assert_eq!(
        resolved.dependencies["rkyv"]["features"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(resolved.dependencies["rkyv"].get("optional").is_none());
    assert!(resolved.features.contains_key("rkyv"));
    assert_eq!(
        resolved.features["json"].as_array().unwrap(),
        &[Value::String("serde_json/raw_value".into())]
    );
    let conflicting = resolve(
        &dir.path().join("notebook.rs"),
        "//! ```cargo\n//! [dependencies]\n//! rkyv = { package = \"unrelated\", version = \"0.8\" }\n//! ```",
    );
    assert!(conflicting.with_runtime(&config, dir.path()).is_err());
    let conflicting = resolve(
        &dir.path().join("notebook.rs"),
        "//! ```cargo\n//! [dependencies]\n//! rkyv = \"0.9\"\n//! ```",
    );
    assert!(conflicting.with_runtime(&config, dir.path()).is_err());
}

#[test]
fn edition_2021_keeps_workspace_defaults_when_member_disables_them() {
    let (dir, notebook) = workspace();
    let root = dir.path().join("Cargo.toml");
    fs::write(
        &root,
        fs::read_to_string(&root)
            .unwrap()
            .replace("default-features = false, ", ""),
    )
    .unwrap();
    let member = dir.path().join("member/Cargo.toml");
    fs::write(
        &member,
        fs::read_to_string(&member).unwrap().replace(
            "workspace = true",
            "workspace = true\ndefault-features = false",
        ),
    )
    .unwrap();
    let resolved = resolve(&notebook, "");
    assert!(
        resolved.dependencies["internal-alias"]
            .get("default-features")
            .is_none()
    );
}

#[test]
fn omitted_build_dependency_features_do_not_break_runtime_manifest() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "app",
        "app",
        r#"
[build-dependencies]
cc = { version = "1", optional = true }
[features]
native = ["dep:cc", "cc/parallel", "cc?/parallel"]
"#,
    );
    let resolved = resolve(&dir.path().join("app/notebook.rs"), "");
    assert!(resolved.features["native"].as_array().unwrap().is_empty());
    assert!(!resolved.dependencies.contains_key("cc"));
}

#[test]
fn mandatory_notebook_override_retains_valid_parent_activation_graph() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "app",
        "app",
        r#"
[dependencies]
helper = { version = "1", optional = true }
[features]
activate = ["dep:helper"]
weak = ["helper?/enabled"]
"#,
    );
    let resolved = resolve(
        &dir.path().join("app/notebook.rs"),
        "//! ```cargo\n//! [dependencies]\n//! helper = \"1\"\n//! ```",
    );
    assert!(resolved.features["activate"].as_array().unwrap().is_empty());
    assert_eq!(
        resolved.features["weak"].as_array().unwrap(),
        &[Value::String("helper/enabled".into())]
    );
    assert!(resolved.reexports().contains("pub use helper;"));
    assert!(!resolved.reexports().contains("#[cfg"));
}

#[test]
fn mandatory_target_override_retains_valid_parent_activation_graph() {
    let dir = tempfile::tempdir().unwrap();
    package(
        dir.path(),
        "app",
        "app",
        r#"
[target.'cfg(unix)'.dependencies]
helper = { version = "1", optional = true }
[features]
activate = ["dep:helper"]
weak = ["helper?/enabled"]
"#,
    );
    let resolved = resolve(
        &dir.path().join("app/notebook.rs"),
        r#"
//! ```cargo
//! [target.'cfg(unix)'.dependencies]
//! helper = "1"
//! ```
"#,
    );
    assert!(resolved.features["activate"].as_array().unwrap().is_empty());
    assert_eq!(
        resolved.features["weak"].as_array().unwrap(),
        &[Value::String("helper/enabled".into())]
    );
}
