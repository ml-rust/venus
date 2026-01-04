//! Test program that loads both libraries and verifies ABI compatibility.

use std::path::PathBuf;
use venus_core::compile::types::{dylib_extension, dylib_prefix};

fn main() {
    println!("=== Venus Cranelift ABI Compatibility Test ===\n");

    // Build platform-specific library names
    let universe_name = format!("{}universe.{}", dylib_prefix(), dylib_extension());
    let cell_name = format!("{}cell.{}", dylib_prefix(), dylib_extension());

    let universe_path = PathBuf::from(&universe_name);
    let cell_path = PathBuf::from(&cell_name);

    if !universe_path.exists() {
        eprintln!("Error: {} not found. Run the build script first.", universe_name);
        std::process::exit(1);
    }

    if !cell_path.exists() {
        eprintln!("Error: {} not found. Run the build script first.", cell_name);
        std::process::exit(1);
    }

    unsafe {
        // Load Universe (LLVM-compiled)
        println!("Loading Universe (LLVM-compiled)...");
        let universe = libloading::Library::new(universe_path).expect("Failed to load universe");
        println!("  ✓ Universe loaded");

        // Load Cell (Cranelift-compiled)
        println!("Loading Cell (Cranelift-compiled)...");
        let cell = libloading::Library::new(cell_path).expect("Failed to load cell");
        println!("  ✓ Cell loaded");

        // Test 1: Simple computation (no cross-library calls)
        println!("\nTest 1: Simple computation in Cranelift cell");
        let cell_compute: libloading::Symbol<extern "C" fn(i64, i64) -> i64> =
            cell.get(b"cell_compute").expect("Failed to get cell_compute");
        let result = cell_compute(10, 20);
        assert_eq!(result, 50, "Expected 10 + 20*2 = 50, got {}", result);
        println!("  ✓ cell_compute(10, 20) = {} (expected 50)", result);

        // Test 2: Cross-library call (Cranelift → LLVM)
        println!("\nTest 2: Cross-library call (Cranelift cell → LLVM universe)");
        let cell_execute: libloading::Symbol<extern "C" fn() -> usize> =
            cell.get(b"cell_execute").expect("Failed to get cell_execute");
        let rows = cell_execute();
        assert_eq!(rows, 1000, "Expected 1000 rows, got {}", rows);
        println!("  ✓ cell_execute() = {} rows (expected 1000)", rows);

        // Test 3: Direct Universe call
        println!("\nTest 3: Direct Universe call");
        let create_df: libloading::Symbol<extern "C" fn(*const u8, usize, usize, usize) -> *mut ()> =
            universe.get(b"universe_create_dataframe").expect("Failed to get universe_create_dataframe");
        let get_rows: libloading::Symbol<extern "C" fn(*const ()) -> usize> =
            universe.get(b"universe_dataframe_rows").expect("Failed to get universe_dataframe_rows");
        let free_df: libloading::Symbol<extern "C" fn(*mut ())> =
            universe.get(b"universe_free_dataframe").expect("Failed to get universe_free_dataframe");

        let name = "direct_test";
        let df = create_df(name.as_ptr(), name.len(), 500, 5);
        let rows = get_rows(df);
        free_df(df);
        assert_eq!(rows, 500, "Expected 500 rows, got {}", rows);
        println!("  ✓ Direct DataFrame creation: {} rows (expected 500)", rows);
    }

    println!("\n=== All tests passed! ===");
    println!("\nCranelift and LLVM produce ABI-compatible code on this system.");
}
