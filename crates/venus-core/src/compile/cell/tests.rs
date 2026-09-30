use super::*;
use crate::graph::{CellId, Dependency, SourceSpan};
use std::path::Path;
use std::process::Command;

fn build_universe(toolchain: &ToolchainManager, build_dir: &Path, value: i32) {
    let release_dir = build_dir.join("target/release");
    fs::create_dir_all(release_dir.join("deps")).unwrap();
    let source = build_dir.join("universe.rs");
    fs::write(&source, format!("pub fn value() -> i32 {{ {value} }}")).unwrap();
    let output = Command::new(toolchain.rustc_path())
        .arg(&source)
        .args([
            "--crate-name=venus_universe",
            "--crate-type=rlib",
            "--edition=2021",
            "-o",
        ])
        .arg(release_dir.join("libvenus_universe.rlib"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dependency_changes_load_distinct_libraries_while_retaining_previous_values() {
    let directory = tempfile::tempdir().unwrap();
    let config = CompilerConfig {
        build_dir: directory.path().join("build"),
        cache_dir: directory.path().join("cache"),
        use_cranelift: false,
        ..CompilerConfig::default()
    };
    let toolchain = ToolchainManager::new().unwrap();
    let universe_dir = config.universe_build_dir();
    build_universe(&toolchain, &universe_dir, 40);
    let universe_path = universe_dir.join(format!(
        "{}venus_universe.{}",
        dylib_prefix(),
        dylib_extension()
    ));
    let compiler = CellCompiler::new(config, toolchain).with_universe(universe_path);
    let cell = make_test_cell();
    let source_hash = compiler.hash_source(&cell.source_code);
    let wrapper = "extern crate venus_universe;\n#[no_mangle]\npub extern \"C\" fn cell_value() -> i32 { venus_universe::value() }";
    let first_path = compiler
        .compile_to_dylib(&cell, wrapper, source_hash, 1)
        .unwrap();
    unsafe {
        let first_library = libloading::Library::new(&first_path).unwrap();
        let first_value: libloading::Symbol<extern "C" fn() -> i32> =
            first_library.get(b"cell_value").unwrap();
        assert_eq!(first_value(), 40);

        build_universe(&compiler.toolchain, &universe_dir, 80);
        let second_path = compiler
            .compile_to_dylib(&cell, wrapper, source_hash, 2)
            .unwrap();
        assert_ne!(first_path, second_path);
        let second_library = libloading::Library::new(&second_path).unwrap();
        let second_value: libloading::Symbol<extern "C" fn() -> i32> =
            second_library.get(b"cell_value").unwrap();
        assert_eq!(second_value(), 80);
        assert_eq!(first_value(), 40);
    }
}

fn make_test_cell() -> CellInfo {
    CellInfo {
        id: CellId::new(0),
        name: "test_cell".to_string(),
        display_name: "test_cell".to_string(),
        dependencies: vec![],
        return_type: "i32".to_string(),
        doc_comment: None,
        source_code: "pub fn test_cell() -> i32 { 42 }".to_string(),
        source_file: PathBuf::from("test.rs"),
        span: SourceSpan {
            start_line: 1,
            start_col: 0,
            end_line: 1,
            end_col: 30,
        },
    }
}

#[test]
fn test_generate_wrapper_simple() {
    let config = CompilerConfig::default();
    let toolchain = ToolchainManager::new().unwrap();
    let compiler = CellCompiler::new(config, toolchain);

    let cell = make_test_cell();
    let wrapper = compiler.generate_wrapper(&cell);

    assert!(wrapper.contains("venus_cell_test_cell"));
    assert!(wrapper.contains("pub fn test_cell() -> i32"));
    assert!(wrapper.contains("#[no_mangle]"));
}

#[test]
fn test_generate_wrapper_with_deps() {
    let config = CompilerConfig::default();
    let toolchain = ToolchainManager::new().unwrap();
    let compiler = CellCompiler::new(config, toolchain);

    let cell = CellInfo {
        id: CellId::new(1),
        name: "process".to_string(),
        display_name: "process".to_string(),
        dependencies: vec![Dependency {
            param_name: "config".to_string(),
            param_type: "Config".to_string(),
            is_ref: true,
            is_mut: false,
        }],
        return_type: "Output".to_string(),
        doc_comment: None,
        source_code: "pub fn process(config: &Config) -> Output { todo!() }".to_string(),
        source_file: PathBuf::from("test.rs"),
        span: SourceSpan {
            start_line: 5,
            start_col: 0,
            end_line: 5,
            end_col: 50,
        },
    };

    let wrapper = compiler.generate_wrapper(&cell);

    assert!(wrapper.contains("config_ptr: *const u8"));
    assert!(wrapper.contains("config_len: usize"));
    assert!(wrapper.contains("rkyv::access"));
}

#[test]
fn test_hash_source() {
    let config = CompilerConfig::default();
    let toolchain = ToolchainManager::new().unwrap();
    let compiler = CellCompiler::new(config, toolchain);

    let hash1 = compiler.hash_source("fn foo() {}");
    let hash2 = compiler.hash_source("fn foo() {}");
    let hash3 = compiler.hash_source("fn bar() {}");

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
}
