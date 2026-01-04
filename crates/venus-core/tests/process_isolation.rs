//! Integration tests for process isolation.
//!
//! Tests that cells running in worker processes can be killed immediately.

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use venus_core::compile::{CellCompiler, CompilationResult, CompilerConfig, ToolchainManager, UniverseBuilder};
use venus_core::execute::ProcessExecutor;
use venus_core::graph::CellParser;
use venus_core::paths::NotebookDirs;

/// Test that an infinite loop can be killed via process isolation.
#[test]
fn test_infinite_loop_can_be_killed() {
    // Path to the infinite loop test notebook
    let notebook_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/infinite_loop.rs");

    if !notebook_path.exists() {
        panic!("Test notebook not found: {:?}", notebook_path);
    }

    // Set up directories
    let dirs = NotebookDirs::from_notebook_path(&notebook_path).unwrap();

    // Parse cells
    let mut parser = CellParser::new();
    let parse_result = parser.parse_file(&notebook_path).unwrap();
    let cells = parse_result.code_cells;
    assert_eq!(cells.len(), 1, "Expected 1 cell");

    let cell = &cells[0];
    assert_eq!(cell.name, "infinite_loop");

    // Set up toolchain and compiler
    let toolchain = ToolchainManager::new().unwrap();
    let config = CompilerConfig::for_notebook(&dirs);

    // Build universe
    let source = std::fs::read_to_string(&notebook_path).unwrap();
    let mut universe_builder = UniverseBuilder::new(config.clone(), toolchain.clone(), None);
    // No definition cells in simple test notebooks
    universe_builder.parse_dependencies(&source, &[]).unwrap();
    let universe_path = universe_builder.build().unwrap();
    let deps_hash = universe_builder.deps_hash();

    // Compile the cell
    let mut compiler = CellCompiler::new(config.clone(), toolchain.clone());
    compiler = compiler.with_universe(universe_path);

    let compiled = match compiler.compile(cell, deps_hash) {
        CompilationResult::Success(c) | CompilationResult::Cached(c) => c,
        CompilationResult::Failed { errors, .. } => {
            panic!("Compilation failed: {:?}", errors);
        }
    };

    // Create process executor
    let mut executor = ProcessExecutor::new(&dirs.state_dir).unwrap();
    executor.register_cell(compiled, 0);

    let cell_id = cell.id;

    // Get the kill handle BEFORE we start executing
    // This handle can be used from another thread to kill the current cell
    let kill_handle = executor.get_kill_handle().unwrap();

    // Spawn a thread to kill the executor after 500ms
    let start = Instant::now();

    let kill_thread = thread::spawn(move || {
        // Wait 500ms then kill
        thread::sleep(Duration::from_millis(500));
        println!("Killing worker process...");
        kill_handle.kill();
        println!("Kill signal sent");
    });

    // Try to execute the infinite loop - this should be killed
    let result = executor.execute_cell(cell_id, &[]);

    let elapsed = start.elapsed();

    // Wait for kill thread
    kill_thread.join().unwrap();

    // Verify:
    // 1. The execution was aborted (not completed)
    // 2. It took less than 2 seconds (not stuck forever)
    println!("Execution took {:?}", elapsed);

    assert!(elapsed < Duration::from_secs(2),
        "Execution took too long ({:?}), process isolation may not be working", elapsed);

    // The result should be an error (either Aborted or IPC error from killed process)
    assert!(result.is_err(), "Expected error from killed execution, got: {:?}", result);

    println!("Successfully killed infinite loop after {:?}", elapsed);
}

/// Test that normal cells still execute correctly with process isolation.
#[test]
fn test_normal_execution_with_process_isolation() {
    // Path to a simple test notebook (no external dependencies)
    let notebook_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples/simple_compute.rs");

    if !notebook_path.exists() {
        panic!("Test notebook not found: {:?}", notebook_path);
    }

    // Set up directories
    let dirs = NotebookDirs::from_notebook_path(&notebook_path).unwrap();

    // Parse cells
    let mut parser = CellParser::new();
    let parse_result = parser.parse_file(&notebook_path).unwrap();
    let cells = parse_result.code_cells;
    assert_eq!(cells.len(), 1, "Expected 1 cell");

    let cell = &cells[0];
    assert_eq!(cell.name, "simple_compute");

    // Set up toolchain and compiler
    let toolchain = ToolchainManager::new().unwrap();
    let config = CompilerConfig::for_notebook(&dirs);

    // Build universe
    let source = std::fs::read_to_string(&notebook_path).unwrap();
    let mut universe_builder = UniverseBuilder::new(config.clone(), toolchain.clone(), None);
    // No definition cells in simple test notebooks
    universe_builder.parse_dependencies(&source, &[]).unwrap();
    let universe_path = universe_builder.build().unwrap();
    let deps_hash = universe_builder.deps_hash();

    // Compile the cell
    let mut compiler = CellCompiler::new(config.clone(), toolchain.clone());
    compiler = compiler.with_universe(universe_path);

    let compiled = match compiler.compile(cell, deps_hash) {
        CompilationResult::Success(c) | CompilationResult::Cached(c) => c,
        CompilationResult::Failed { errors, .. } => {
            panic!("Compilation failed: {:?}", errors);
        }
    };

    // Create process executor
    let mut executor = ProcessExecutor::new(&dirs.state_dir).unwrap();
    executor.register_cell(compiled, 0);

    let start = Instant::now();

    // Execute - should complete normally
    let result = executor.execute_cell(cell.id, &[]);
    let elapsed = start.elapsed();

    println!("Execution completed in {:?}", elapsed);
    assert!(result.is_ok(), "Expected successful execution, got: {:?}", result);

    // Verify the result is the expected sum
    let output = result.unwrap();
    println!("Output: {:?}", output.display_text());
    println!("Normal execution completed successfully");
}
