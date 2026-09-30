//! Generated notebook projects retain Cargo workspace dependency semantics.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;
use venus_core::compile::{
    CompilerConfig, ProductionBuilder, ToolchainManager, UniverseBuilder, find_notebook_manifest,
};

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn package(root: &Path, directory: &str, name: &str, manifest: &str, source: &str) {
    write(
        root,
        &format!("{directory}/Cargo.toml"),
        &format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2021\"\n{manifest}"),
    );
    write(root, &format!("{directory}/src/lib.rs"), source);
}

fn runtime(root: &Path) -> PathBuf {
    let mut source = String::new();
    for function in [
        "input_slider",
        "input_slider_with_step",
        "input_slider_labeled",
        "input_text",
        "input_text_with_default",
        "input_text_labeled",
        "input_select",
        "input_select_labeled",
        "input_checkbox",
        "input_checkbox_labeled",
    ] {
        source.push_str(&format!("pub fn {function}() {{}}\n"));
    }
    source.push_str("pub mod widgets { pub struct WidgetContext; pub struct WidgetValue; pub struct WidgetDef; pub fn set_widget_context() {} pub fn take_widget_context() {} }\n");
    package(root, "runtime", "venus", "", &source);
    root.join("runtime")
}

fn config(root: &Path, runtime: PathBuf) -> CompilerConfig {
    CompilerConfig {
        build_dir: root.join("artifacts/build"),
        cache_dir: root.join("artifacts/cache"),
        venus_crate_path: Some(runtime),
        use_cranelift: false,
        ..CompilerConfig::default()
    }
}

fn host() -> String {
    let output = Command::new("rustc").arg("-vV").output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap()
        .to_string()
}

fn cell_value(config: &CompilerConfig, root: &Path, expression: &str, deps_hash: u64) -> i32 {
    let extension = std::env::consts::DLL_EXTENSION;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    expression.hash(&mut hasher);
    let expression_hash = hasher.finish();
    let cell_path = root.join(format!(
        "cell_{expression_hash:x}_{deps_hash:x}.{extension}"
    ));
    let source = root.join("cell.rs");
    fs::write(&source, format!("extern crate venus_universe;\nuse venus_universe::*;\n#[no_mangle] pub extern \"C\" fn cell_value() -> i32 {{ {expression} }}")).unwrap();
    let output = Command::new("rustc")
        .args(["--edition=2021", "--crate-type=cdylib", "--extern"])
        .arg(format!(
            "venus_universe={}",
            config
                .universe_build_dir()
                .join("target/release/libvenus_universe.rlib")
                .display()
        ))
        .arg("-L")
        .arg(format!(
            "dependency={}",
            config
                .universe_build_dir()
                .join("target/release/deps")
                .display()
        ))
        .arg(&source)
        .arg("-o")
        .arg(&cell_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    unsafe {
        let library = libloading::Library::new(cell_path).unwrap();
        let function: libloading::Symbol<extern "C" fn() -> i32> =
            library.get(b"cell_value").unwrap();
        function()
    }
}

fn fixture() -> (TempDir, PathBuf, CompilerConfig) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let runtime_path = runtime(root);
    package(
        root,
        "helper",
        "internal-helper",
        "[features]\ndefault = [\"default-on\"]\ndefault-on = []\nbase = []\nextra = []\n",
        "#[cfg(feature = \"default-on\")] compile_error!(\"workspace defaults must stay disabled\"); pub fn value() -> i32 { 40 }",
    );
    package(
        root,
        "optional",
        "optional-helper",
        "[features]\nenabled = []\n",
        "#[cfg(feature = \"enabled\")] pub fn value() -> i32 { 2 }\n#[cfg(not(feature = \"enabled\"))] pub fn value() -> i32 { 200 }",
    );
    package(
        root,
        "active",
        "active-helper",
        "",
        "pub fn value() -> i32 { 1 }",
    );
    package(
        root,
        "blocked",
        "blocked-helper",
        "",
        "compile_error!(\"inactive dependency must stay disabled\");",
    );
    package(
        root,
        "inactive",
        "inactive-helper",
        "",
        "compile_error!(\"inactive target must stay disabled\");",
    );
    package(
        root,
        "patch",
        "patched-helper",
        "",
        "pub fn value() -> i32 { 3 }",
    );
    write(
        root,
        "Cargo.toml",
        r#"
[workspace]
members = ["member", "runtime", "helper", "optional", "active", "blocked", "inactive", "patch"]
[workspace.dependencies]
internal-alias = { package = "internal-helper", path = "helper", default-features = false, features = ["base"] }
unused = "999.0"
[patch.crates-io]
patched-helper = { path = "patch" }
"#,
    );
    let member_manifest = format!(
        r#"
[dependencies]
internal-alias = {{ workspace = true, features = ["extra"] }}
optional-alias = {{ package = "optional-helper", path = "../optional", optional = true }}
blocked = {{ package = "blocked-helper", path = "../blocked", optional = true }}
patched-helper = "0.1"
serde_json = {{ version = "1.0", optional = true }}
[target.{host:?}.dependencies]
active-alias = {{ package = "active-helper", path = "../active" }}
[target.'cfg(target_os = "none")'.dependencies]
inactive = {{ package = "inactive-helper", path = "../inactive" }}
[features]
default = ["outer"]
outer = ["activate", "json"]
activate = ["dep:optional-alias", "optional-alias/enabled"]
json = ["dep:serde_json", "serde_json?/raw_value"]
weak = ["optional-alias?/enabled"]
"#,
        host = host()
    );
    package(root, "member", "member", &member_manifest, "");
    let notebook = root.join("member/notebooks/report.rs");
    write(
        root,
        "member/notebooks/report.rs",
        "#[venus::cell]\npub fn answer() -> i32 { internal_alias::value() + active_alias::value() + optional_alias::value() + patched_helper::value() }\n",
    );
    let config = config(root, runtime_path);
    (dir, notebook, config)
}

#[test]
fn generated_universe_and_standalone_binary_execute_workspace_aliases() {
    let (dir, notebook, config) = fixture();
    assert_eq!(
        find_notebook_manifest(&notebook),
        Some(dir.path().join("member/Cargo.toml").canonicalize().unwrap())
    );
    let mut universe =
        UniverseBuilder::for_notebook(config.clone(), ToolchainManager::new().unwrap(), &notebook)
            .unwrap();
    let source = fs::read_to_string(&notebook).unwrap();
    universe.parse_dependencies(&source, &[]).unwrap();
    universe.build().unwrap();
    assert_eq!(
        cell_value(
            &config,
            dir.path(),
            "internal_alias::value() + active_alias::value() + optional_alias::value() + patched_helper::value()",
            universe.deps_hash()
        ),
        46
    );
    let manifest: toml::Table = toml::from_str(
        &fs::read_to_string(config.universe_build_dir().join("Cargo.toml")).unwrap(),
    )
    .unwrap();
    assert!(manifest.contains_key("workspace"));
    assert!(
        !manifest["dependencies"]
            .as_table()
            .unwrap()
            .contains_key("unused")
    );
    for (alias, directory) in [
        ("internal-alias", "helper"),
        ("optional-alias", "optional"),
        ("blocked", "blocked"),
        ("venus", "runtime"),
    ] {
        let expected = dir.path().join(directory).canonicalize().unwrap();
        assert_eq!(
            manifest["dependencies"][alias]["path"].as_str(),
            Some(expected.to_string_lossy().as_ref())
        );
    }
    for (alias, directory) in [("active-alias", "active"), ("inactive", "inactive")] {
        let dependency = manifest["target"]
            .as_table()
            .unwrap()
            .values()
            .find_map(|target| target["dependencies"].get(alias))
            .unwrap();
        let expected = dir.path().join(directory).canonicalize().unwrap();
        assert_eq!(
            dependency["path"].as_str(),
            Some(expected.to_string_lossy().as_ref())
        );
    }
    let expected = dir.path().join("patch").canonicalize().unwrap();
    assert_eq!(
        manifest["patch"]["crates-io"]["patched-helper"]["path"].as_str(),
        Some(expected.to_string_lossy().as_ref())
    );
    assert!(!universe.is_cache_valid());
    let previous_hash = universe.deps_hash();
    fs::write(
        dir.path().join("helper/src/lib.rs"),
        "pub fn value() -> i32 { 80 }",
    )
    .unwrap();
    universe.parse_dependencies(&source, &[]).unwrap();
    assert_ne!(previous_hash, universe.deps_hash());
    universe.build().unwrap();
    assert_eq!(
        cell_value(
            &config,
            dir.path(),
            "internal_alias::value() + active_alias::value() + optional_alias::value() + patched_helper::value()",
            universe.deps_hash()
        ),
        86
    );

    let mut production = ProductionBuilder::new(config.clone());
    production.load(&notebook).unwrap();
    let binary = dir.path().join("report-bin");
    production.build(&binary, false).unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("86"));

    // Disabling default activation must remove optional exports and preserve target exports.
    let manifest = dir.path().join("member/Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("default = [\"outer\"]", "default = []"),
    )
    .unwrap();
    universe.parse_dependencies(&source, &[]).unwrap();
    universe.build().unwrap();
    assert_eq!(
        cell_value(
            &config,
            dir.path(),
            "internal_alias::value() + active_alias::value() + patched_helper::value()",
            universe.deps_hash()
        ),
        84
    );
    let generated = fs::read_to_string(config.universe_build_dir().join("src/lib.rs")).unwrap();
    assert!(generated.contains("pub use optional_alias;"));
    assert!(generated.contains("#[cfg(any(any("));
}

#[test]
fn standalone_notebook_and_direct_builder_generate_runtime_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let runtime_path = runtime(dir.path());
    let config = config(dir.path(), runtime_path);
    let builder = UniverseBuilder::new(config.clone(), ToolchainManager::new().unwrap(), None);
    let dependency_hash = builder.deps_hash();
    builder.build().unwrap();
    assert_eq!(builder.deps_hash(), dependency_hash);
    assert_eq!(
        fs::read_to_string(config.cache_dir.join("universe_hash")).unwrap(),
        dependency_hash.to_string()
    );
    let manifest: toml::Table = toml::from_str(
        &fs::read_to_string(config.universe_build_dir().join("Cargo.toml")).unwrap(),
    )
    .unwrap();
    assert!(manifest["dependencies"].get("rkyv").is_some());
    assert!(manifest["dependencies"].get("serde_json").is_some());
    let notebook = dir.path().join("standalone.rs");
    fs::write(&notebook, "#[venus::cell]\npub fn answer() -> i32 { 7 }\n").unwrap();
    let mut production = ProductionBuilder::new(config);
    production.load(&notebook).unwrap();
    let binary = dir.path().join("standalone-bin");
    production.build(&binary, false).unwrap();
    let output = Command::new(binary).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("7"));
}
