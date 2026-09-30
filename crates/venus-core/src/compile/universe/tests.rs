use super::UniverseBuilder;
use crate::compile::{CompilerConfig, ToolchainManager};
use crate::graph::{CellId, DefinitionCell, DefinitionType, SourceSpan};
use std::path::PathBuf;

fn make_builder() -> UniverseBuilder {
    let config = CompilerConfig::default();
    let toolchain = ToolchainManager::new().unwrap();
    UniverseBuilder::new(config, toolchain, None)
}

/// Build a definition cell of the given type with the given content.
fn def_cell(content: &str, definition_type: DefinitionType) -> DefinitionCell {
    DefinitionCell {
        id: CellId::new(0),
        content: content.to_string(),
        definition_type,
        span: SourceSpan {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: content.len(),
        },
        source_file: PathBuf::from("notebook.rs"),
        doc_comment: None,
    }
}

/// Assert that a notebook `use` statement referencing `path_fragment` appears
/// in the generated universe and is re-exported (`pub use`), never left as a
/// private `use`. A private `use` in the universe crate is invisible to cells
/// (which link the universe and glob-import it), which is exactly the failure
/// where a notebook import "doesn't work" for a cell that names the type.
fn assert_import_reexported(lib: &str, path_fragment: &str) {
    // The universe is rendered from `syn` tokens, which space out `::`, so
    // compare with all whitespace removed.
    let strip = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let needle = strip(path_fragment);

    let matching: Vec<&str> = lib
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.contains("use") && strip(l).contains(&needle))
        .collect();

    assert!(
        !matching.is_empty(),
        "expected an import referencing `{path_fragment}` in generated universe, found none:\n{lib}"
    );

    for line in matching {
        assert!(
            strip(line).starts_with("pubuse"),
            "notebook import `{line}` must be re-exported as `pub use` so cells can see it, \
                 but it was emitted as a private `use` (invisible through `use venus_universe::*`)"
        );
    }
}

#[test]
fn universe_reexports_named_import_from_definition_cell() {
    let mut builder = make_builder();
    let cells = vec![def_cell(
        "use plotlars::polars::prelude::DataFrame;",
        DefinitionType::Import,
    )];
    builder.parse_dependencies("", &cells).unwrap();

    let lib = builder.generate_lib_rs();
    assert_import_reexported(&lib, "plotlars::polars::prelude::DataFrame");
}

#[test]
fn universe_reexports_glob_import_from_definition_cell() {
    let mut builder = make_builder();
    let cells = vec![def_cell("use venus::prelude::*;", DefinitionType::Import)];
    builder.parse_dependencies("", &cells).unwrap();

    let lib = builder.generate_lib_rs();
    assert_import_reexported(&lib, "venus::prelude::*");
}

#[test]
fn universe_reexports_aliased_import_from_definition_cell() {
    let mut builder = make_builder();
    let cells = vec![def_cell(
        "use std::collections::HashMap as Map;",
        DefinitionType::Import,
    )];
    builder.parse_dependencies("", &cells).unwrap();

    let lib = builder.generate_lib_rs();
    assert_import_reexported(&lib, "std::collections::HashMap as Map");
}

#[test]
fn universe_reexports_multiple_imports_in_one_cell() {
    let mut builder = make_builder();
    let cells = vec![def_cell(
        "use std::collections::HashMap;\nuse std::collections::BTreeMap;",
        DefinitionType::Import,
    )];
    builder.parse_dependencies("", &cells).unwrap();

    let lib = builder.generate_lib_rs();
    assert_import_reexported(&lib, "std::collections::HashMap");
    assert_import_reexported(&lib, "std::collections::BTreeMap");
}

#[test]
fn universe_reexports_use_from_mixed_definition_cell() {
    let mut builder = make_builder();
    // A single definition cell that mixes a `use` with a type definition is
    // classified as `Mixed`; its import must still reach cells, and its type
    // must still be present.
    let cells = vec![def_cell(
        "use std::collections::HashMap;\n\npub struct Config {\n    pub map: HashMap<String, i32>,\n}",
        DefinitionType::Mixed,
    )];
    builder.parse_dependencies("", &cells).unwrap();

    let lib = builder.generate_lib_rs();
    assert_import_reexported(&lib, "std::collections::HashMap");
    assert!(
        lib.contains("struct Config"),
        "type definition from a mixed cell must still be present in the universe:\n{lib}"
    );
}

#[test]
fn test_parse_simple_dependency() {
    let mut builder = make_builder();

    let source = r#"
//! # My Notebook
//!
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```

#[venus::cell]
pub fn hello() -> i32 { 42 }
"#;

    builder.parse_dependencies(source, &[]).unwrap();

    assert_eq!(builder.dependencies().len(), 1);
    assert_eq!(builder.dependencies()[0].name, "serde");
    assert_eq!(builder.dependencies()[0].version, Some("1.0".to_string()));
}

#[test]
fn test_parse_complex_dependency() {
    let mut builder = make_builder();

    let source = r#"
//! ```cargo
//! [dependencies]
//! tokio = { version = "1", features = ["full"] }
//! ```
"#;

    builder.parse_dependencies(source, &[]).unwrap();

    assert_eq!(builder.dependencies().len(), 1);
    assert_eq!(builder.dependencies()[0].name, "tokio");
    assert_eq!(builder.dependencies()[0].version, Some("1".to_string()));
    assert_eq!(builder.dependencies()[0].features, vec!["full"]);
}

#[test]
fn test_parse_multiple_dependencies() {
    let mut builder = make_builder();

    let source = r#"
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! serde_json = "1.0"
//! tokio = { version = "1", features = ["rt", "macros"] }
//! ```
"#;

    builder.parse_dependencies(source, &[]).unwrap();

    assert_eq!(builder.dependencies().len(), 3);
}

#[test]
fn test_generate_cargo_toml() {
    let mut builder = make_builder();

    // Parse a dependency with features
    let source = r#"
//! ```cargo
//! [dependencies]
//! serde = { version = "1.0", features = ["derive"] }
//! ```
"#;
    builder.parse_dependencies(source, &[]).unwrap();

    let toml = builder.generate_cargo_toml().unwrap();
    assert!(toml.contains("[package]"));
    assert!(toml.contains("venus_universe"));
    assert!(toml.contains("serde"));
    assert!(toml.contains("derive"));
}

#[test]
fn test_hash_changes_with_deps() {
    let mut builder = make_builder();

    builder.parse_dependencies("", &[]).unwrap();
    let hash1 = builder.deps_hash();

    builder
        .parse_dependencies(
            r#"
//! ```cargo
//! [dependencies]
//! serde = "1.0"
//! ```
"#,
            &[],
        )
        .unwrap();
    let hash2 = builder.deps_hash();

    assert_ne!(hash1, hash2);
}

#[test]
fn notebook_override_clears_parent_runtime_conflict_after_construction() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("Cargo.toml"),
        r#"
[package]
name = "parent"
version = "0.1.0"
[dependencies]
rkyv = "0.7"
"#,
    )
    .unwrap();
    let source = r#"
//! ```cargo
//! [dependencies]
//! rkyv = "0.8"
//! ```
"#;
    let notebook = directory.path().join("notebook.rs");
    std::fs::write(&notebook, source).unwrap();
    let config = CompilerConfig {
        venus_crate_path: None,
        ..CompilerConfig::default()
    };
    let mut builder =
        UniverseBuilder::for_notebook(config, ToolchainManager::new().unwrap(), &notebook).unwrap();
    assert!(builder.build().unwrap_err().to_string().contains("rkyv"));
    builder.parse_dependencies(source, &[]).unwrap();
    let manifest: toml::Table = toml::from_str(&builder.generate_cargo_toml().unwrap()).unwrap();
    let requirement = semver::VersionReq::parse(
        manifest["dependencies"]["rkyv"]["version"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(requirement.matches(&semver::Version::new(0, 8, 12)));
    assert!(!requirement.matches(&semver::Version::new(0, 7, 0)));
    assert!(builder.dependencies().iter().any(
        |dependency| dependency.name == "rkyv" && dependency.version.as_deref() == Some("0.8")
    ));
}
