//! Integration tests for Cranelift ABI compatibility and hot-reload.

use std::path::PathBuf;
use std::process::Command;

/// Get the path to the cranelift validation test directory
fn test_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("cranelift_validation")
}

/// Compile a Rust source file to a cdylib using LLVM
fn compile_llvm(src: &str, output: &str) -> bool {
    let dir = test_dir();
    Command::new("rustc")
        .current_dir(&dir)
        .args([
            "--edition",
            "2021",
            "--crate-type",
            "cdylib",
            "-o",
            output,
            src,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Compile a Rust source file to a cdylib using Cranelift
fn compile_cranelift(src: &str, output: &str) -> bool {
    let dir = test_dir();
    Command::new("rustup")
        .current_dir(&dir)
        .args([
            "run",
            "nightly",
            "rustc",
            "--edition",
            "2021",
            "-Zcodegen-backend=cranelift",
            "--crate-type",
            "cdylib",
            "-L",
            ".",
            "-o",
            output,
            src,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn test_cranelift_available() {
    // Check if cranelift backend is available by trying to compile with it
    let dir = test_dir();
    let test_src = dir.join("cranelift_check.rs");
    std::fs::write(
        &test_src,
        "#[no_mangle] pub extern \"C\" fn check() -> u32 { 42 }",
    )
    .unwrap();

    let result = Command::new("rustup")
        .current_dir(&dir)
        .args([
            "run",
            "nightly",
            "rustc",
            "--edition",
            "2021",
            "-Zcodegen-backend=cranelift",
            "--crate-type",
            "cdylib",
            "-o",
            "libcheck.so",
            "cranelift_check.rs",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    // Cleanup
    let _ = std::fs::remove_file(test_src);
    let _ = std::fs::remove_file(dir.join("libcheck.so"));

    assert!(
        result,
        "Cranelift compilation failed. Install with: rustup component add rustc-codegen-cranelift --toolchain nightly"
    );
}

#[test]
fn test_llvm_compilation() {
    let dir = test_dir();
    assert!(dir.join("universe.rs").exists(), "universe.rs not found");

    assert!(
        compile_llvm("universe.rs", "libuniverse_test.so"),
        "LLVM compilation failed"
    );
    assert!(
        dir.join("libuniverse_test.so").exists(),
        "Output library not created"
    );

    // Cleanup
    let _ = std::fs::remove_file(dir.join("libuniverse_test.so"));
}

#[test]
fn test_cranelift_compilation() {
    let dir = test_dir();
    assert!(dir.join("cell.rs").exists(), "cell.rs not found");

    // First compile universe (needed for linking)
    compile_llvm("universe.rs", "libuniverse.so");

    assert!(
        compile_cranelift("cell.rs", "libcell_test.so"),
        "Cranelift compilation failed"
    );
    assert!(
        dir.join("libcell_test.so").exists(),
        "Output library not created"
    );

    // Cleanup
    let _ = std::fs::remove_file(dir.join("libcell_test.so"));
}

#[test]
fn test_load_llvm_library() {
    let dir = test_dir();

    // Compile universe
    assert!(
        compile_llvm("universe.rs", "libuniverse_load.so"),
        "Compilation failed"
    );

    let lib_path = dir.join("libuniverse_load.so");

    unsafe {
        let lib = libloading::Library::new(&lib_path).expect("Failed to load library");

        // Get symbol
        let create_df: libloading::Symbol<
            extern "C" fn(*const u8, usize, usize, usize) -> *mut (),
        > = lib
            .get(b"universe_create_dataframe")
            .expect("Symbol not found");

        let get_rows: libloading::Symbol<extern "C" fn(*const ()) -> usize> = lib
            .get(b"universe_dataframe_rows")
            .expect("Symbol not found");

        let free_df: libloading::Symbol<extern "C" fn(*mut ())> = lib
            .get(b"universe_free_dataframe")
            .expect("Symbol not found");

        // Test the functions
        let name = "test";
        let df = create_df(name.as_ptr(), name.len(), 100, 5);
        assert!(!df.is_null(), "DataFrame creation failed");

        let rows = get_rows(df);
        assert_eq!(rows, 100, "Expected 100 rows");

        free_df(df);
    }

    // Cleanup
    let _ = std::fs::remove_file(lib_path);
}

#[test]
fn test_load_cranelift_library() {
    let dir = test_dir();

    // Create a standalone cell that doesn't need universe
    let standalone_src = dir.join("cell_standalone.rs");
    std::fs::write(
        &standalone_src,
        r#"
        #[no_mangle]
        pub extern "C" fn cell_compute(a: i64, b: i64) -> i64 {
            a + b * 2
        }

        #[no_mangle]
        pub extern "C" fn cell_add_all(arr: *const i64, len: usize) -> i64 {
            let slice = unsafe { std::slice::from_raw_parts(arr, len) };
            slice.iter().sum()
        }
    "#,
    )
    .expect("Failed to write standalone cell");

    // Compile with Cranelift
    let result = Command::new("rustup")
        .current_dir(&dir)
        .args([
            "run",
            "nightly",
            "rustc",
            "--edition",
            "2021",
            "-Zcodegen-backend=cranelift",
            "--crate-type",
            "cdylib",
            "-o",
            "libcell_standalone.so",
            "cell_standalone.rs",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(result, "Cranelift compilation failed");

    let lib_path = dir.join("libcell_standalone.so");

    unsafe {
        let lib = libloading::Library::new(&lib_path).expect("Failed to load Cranelift library");

        // Test simple function
        let compute: libloading::Symbol<extern "C" fn(i64, i64) -> i64> =
            lib.get(b"cell_compute").expect("Symbol not found");

        let result = compute(5, 10);
        assert_eq!(result, 25, "Expected 5 + 10*2 = 25, got {}", result);

        // Test array function
        let add_all: libloading::Symbol<extern "C" fn(*const i64, usize) -> i64> =
            lib.get(b"cell_add_all").expect("Symbol not found");

        let arr = [1i64, 2, 3, 4, 5];
        let sum = add_all(arr.as_ptr(), arr.len());
        assert_eq!(sum, 15, "Expected sum 15, got {}", sum);
    }

    // Cleanup
    let _ = std::fs::remove_file(standalone_src);
    let _ = std::fs::remove_file(lib_path);
}

#[test]
fn test_cross_library_call() {
    let dir = test_dir();

    // Compile universe
    assert!(
        compile_llvm("universe.rs", "libuniverse.so"),
        "Universe compilation failed"
    );

    // Compile cell with explicit link to universe and rpath
    let rpath_arg = format!("-Clink-arg=-Wl,-rpath,{}", dir.display());
    let result = Command::new("rustup")
        .current_dir(&dir)
        .args([
            "run",
            "nightly",
            "rustc",
            "--edition",
            "2021",
            "-Zcodegen-backend=cranelift",
            "--crate-type",
            "cdylib",
            "-L",
            ".",
            "-l",
            "universe",
            &rpath_arg,
            "-o",
            "libcell_cross.so",
            "cell.rs",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(result, "Cell compilation with linking failed");

    let cell_path = dir.join("libcell_cross.so");

    unsafe {
        // Load cell - it should find universe via rpath
        let cell = libloading::Library::new(&cell_path).expect("Failed to load cell");

        // Test cross-library call
        let execute: libloading::Symbol<extern "C" fn() -> usize> =
            cell.get(b"cell_execute").expect("Symbol not found");

        let rows = execute();
        assert_eq!(
            rows, 1000,
            "Expected 1000 rows from cross-library call, got {}",
            rows
        );
    }

    // Cleanup
    let _ = std::fs::remove_file(cell_path);
}

#[test]
fn test_hot_reload() {
    let dir = test_dir();

    // Create a simple hot-reload test source
    let test_src = dir.join("hot_reload_test.rs");
    std::fs::write(
        &test_src,
        r#"
        #[no_mangle]
        pub extern "C" fn get_version() -> u32 { 1 }
    "#,
    )
    .expect("Failed to write test source");

    // Compile version 1
    assert!(
        compile_cranelift("hot_reload_test.rs", "libhot_v1.so"),
        "V1 compilation failed"
    );

    let lib_path = dir.join("libhot_v1.so");

    // Load and verify version 1
    let version1 = unsafe {
        let lib = libloading::Library::new(&lib_path).expect("Failed to load v1");
        let get_ver: libloading::Symbol<extern "C" fn() -> u32> =
            lib.get(b"get_version").expect("Symbol not found");
        let v = get_ver();
        drop(lib); // Unload
        v
    };
    assert_eq!(version1, 1, "Expected version 1");

    // Update source for version 2
    std::fs::write(
        &test_src,
        r#"
        #[no_mangle]
        pub extern "C" fn get_version() -> u32 { 2 }
    "#,
    )
    .expect("Failed to write updated source");

    // Recompile (simulate hot-reload)
    assert!(
        compile_cranelift("hot_reload_test.rs", "libhot_v2.so"),
        "V2 compilation failed"
    );

    let lib_path_v2 = dir.join("libhot_v2.so");

    // Load and verify version 2
    let version2 = unsafe {
        let lib = libloading::Library::new(&lib_path_v2).expect("Failed to load v2");
        let get_ver: libloading::Symbol<extern "C" fn() -> u32> =
            lib.get(b"get_version").expect("Symbol not found");
        get_ver()
    };
    assert_eq!(version2, 2, "Expected version 2 after hot-reload");

    // Cleanup
    let _ = std::fs::remove_file(test_src);
    let _ = std::fs::remove_file(lib_path);
    let _ = std::fs::remove_file(lib_path_v2);
}

#[test]
fn test_hot_reload_preserves_state() {
    use venus_core::graph::CellId;
    use venus_core::state::{BoxedOutput, StateManager};

    let dir = test_dir();
    let state_dir = dir.join("state_hot_reload");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    // Create a StateManager to track cell outputs
    let mut state = StateManager::new(&state_dir).expect("Failed to create StateManager");

    // Create a cell source (version 1)
    let test_src = dir.join("stateful_cell.rs");
    std::fs::write(
        &test_src,
        r#"
        #[no_mangle]
        pub extern "C" fn compute_value(input: i64) -> i64 {
            input * 2  // Version 1: multiply by 2
        }
    "#,
    )
    .expect("Failed to write v1 source");

    // Compile version 1
    assert!(
        compile_cranelift("stateful_cell.rs", "libstateful_v1.so"),
        "V1 compilation failed"
    );

    let lib_path_v1 = dir.join("libstateful_v1.so");
    let cell_id = CellId::new(42);

    // Execute version 1 and store the output
    let output_v1 = unsafe {
        let lib = libloading::Library::new(&lib_path_v1).expect("Failed to load v1");
        let compute: libloading::Symbol<extern "C" fn(i64) -> i64> =
            lib.get(b"compute_value").expect("Symbol not found");
        compute(10) // 10 * 2 = 20
    };
    assert_eq!(output_v1, 20, "V1 should compute 10 * 2 = 20");

    // Store the output in StateManager (simulating cell execution)
    let output_bytes = output_v1.to_le_bytes().to_vec();
    state.store_output(cell_id, BoxedOutput::from_raw_bytes(output_bytes.clone()));

    // Verify state is stored
    let stored = state.get_output(cell_id);
    assert!(stored.is_some(), "Output should be stored before hot-reload");
    assert_eq!(
        stored.unwrap().bytes(),
        &output_bytes[..],
        "Stored bytes should match"
    );

    // Simulate hot-reload: save state before unloading
    let saved_output = state.get_output(cell_id).clone();
    assert!(saved_output.is_some(), "Should have saved output before reload");

    // Update source for version 2 (compatible change - same signature)
    std::fs::write(
        &test_src,
        r#"
        #[no_mangle]
        pub extern "C" fn compute_value(input: i64) -> i64 {
            input * 3  // Version 2: multiply by 3
        }
    "#,
    )
    .expect("Failed to write v2 source");

    // Recompile version 2
    assert!(
        compile_cranelift("stateful_cell.rs", "libstateful_v2.so"),
        "V2 compilation failed"
    );

    let lib_path_v2 = dir.join("libstateful_v2.so");

    // Load version 2 (new behavior)
    let output_v2 = unsafe {
        let lib = libloading::Library::new(&lib_path_v2).expect("Failed to load v2");
        let compute: libloading::Symbol<extern "C" fn(i64) -> i64> =
            lib.get(b"compute_value").expect("Symbol not found");
        compute(10) // 10 * 3 = 30
    };
    assert_eq!(output_v2, 30, "V2 should compute 10 * 3 = 30");

    // KEY TEST: Verify the OLD state is still preserved in StateManager
    // (The saved output from before reload should still be accessible)
    let preserved = state.get_output(cell_id);
    assert!(
        preserved.is_some(),
        "State should be preserved after hot-reload"
    );

    // The preserved state should still contain the V1 output
    // (until explicitly invalidated or re-executed)
    let preserved_output = preserved.unwrap();
    let preserved_bytes = preserved_output.bytes();
    assert_eq!(
        preserved_bytes, &output_bytes[..],
        "Preserved state should still contain V1 output (20) until re-execution"
    );

    // Verify we can access the saved output
    let saved = saved_output.unwrap();
    let saved_value = i64::from_le_bytes(saved.bytes().try_into().unwrap());
    assert_eq!(saved_value, 20, "Saved output should be 20 (V1 result)");

    // Test state invalidation (simulating schema-incompatible change)
    state.invalidate(cell_id);
    assert!(
        state.get_output(cell_id).is_none(),
        "State should be gone after invalidation"
    );

    // Cleanup
    let _ = std::fs::remove_file(test_src);
    let _ = std::fs::remove_file(lib_path_v1);
    let _ = std::fs::remove_file(lib_path_v2);
    let _ = std::fs::remove_dir_all(state_dir);
}
