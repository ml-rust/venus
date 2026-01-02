//! Integration tests for multi-cell notebook execution.
//!
//! Tests the complete workflow from parsing to execution.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use venus_core::compile::{CompilerConfig, ToolchainManager};
use venus_core::graph::{CellId, CellInfo, CellParser, GraphEngine};
use venus_core::state::{BoxedOutput, StateManager};

// =============================================================================
// Test Helpers
// =============================================================================

/// RAII wrapper for test notebook with automatic cleanup.
struct TestNotebook {
    dir: PathBuf,
    path: PathBuf,
}

impl TestNotebook {
    /// Create a new test notebook with the given source.
    fn new(filename: &str, source: &str) -> Self {
        let dir = std::env::temp_dir()
            .join("venus_integration_tests")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).expect("Failed to create test directory");

        let path = dir.join(filename);
        fs::write(&path, source).expect("Failed to write notebook file");

        Self { dir, path }
    }

    /// Parse cells from the notebook.
    fn parse(&self) -> Vec<CellInfo> {
        let mut parser = CellParser::new();
        parser
            .parse_file(&self.path)
            .expect("Failed to parse notebook")
            .code_cells
    }

    /// Build a graph from parsed cells, returning (graph, cell_id_map).
    fn build_graph(&self) -> (GraphEngine, HashMap<String, CellId>) {
        let cells = self.parse();
        let mut graph = GraphEngine::new();
        let mut cell_ids = HashMap::new();

        for cell in &cells {
            let id = graph.add_cell(cell.clone());
            cell_ids.insert(cell.name.clone(), id);
        }

        graph
            .resolve_dependencies()
            .expect("Failed to resolve dependencies");

        (graph, cell_ids)
    }
}

impl Drop for TestNotebook {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// RAII wrapper for test state directory with automatic cleanup.
struct TestStateDir {
    path: PathBuf,
}

impl TestStateDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir()
            .join("venus_integration_tests")
            .join(uuid::Uuid::new_v4().to_string())
            .join(name);
        fs::create_dir_all(&path).expect("Failed to create state directory");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TestStateDir {
    fn drop(&mut self) {
        // Remove parent (uuid dir) to clean up completely
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

/// Create a test notebook source with multiple cells.
fn create_multi_cell_notebook() -> String {
    r#"
//! Test notebook with multiple cells

/// Computes the base value
#[venus::cell]
pub fn base() -> i32 {
    42
}

/// Doubles the base value
#[venus::cell]
pub fn doubled(base: &i32) -> i32 {
    base * 2
}

/// Adds ten to doubled
#[venus::cell]
pub fn plus_ten(doubled: &i32) -> i32 {
    doubled + 10
}
"#
    .to_string()
}

/// Create a diamond dependency notebook.
fn create_diamond_notebook() -> String {
    r#"
//! Test notebook with diamond dependencies

/// Root value
#[venus::cell]
pub fn root() -> i32 {
    10
}

/// Left branch: multiply by 2
#[venus::cell]
pub fn left(root: &i32) -> i32 {
    root * 2
}

/// Right branch: multiply by 3
#[venus::cell]
pub fn right(root: &i32) -> i32 {
    root * 3
}

/// Merge: sum both branches
#[venus::cell]
pub fn merge(left: &i32, right: &i32) -> i32 {
    left + right
}
"#
    .to_string()
}

#[test]
fn test_parse_multi_cell_notebook() {
    let notebook = TestNotebook::new("test_parse.rs", &create_multi_cell_notebook());
    let cells = notebook.parse();

    assert_eq!(cells.len(), 3, "Expected 3 cells");

    // Verify cell names
    let names: Vec<_> = cells.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"base"), "Missing 'base' cell");
    assert!(names.contains(&"doubled"), "Missing 'doubled' cell");
    assert!(names.contains(&"plus_ten"), "Missing 'plus_ten' cell");

    // Verify dependencies
    let doubled = cells
        .iter()
        .find(|c| c.name == "doubled")
        .expect("Fixture must contain 'doubled' cell");
    assert_eq!(
        doubled.dependencies.len(),
        1,
        "doubled should have 1 dependency"
    );
    assert_eq!(
        doubled.dependencies[0].param_name, "base",
        "doubled should depend on base"
    );
    // Cleanup handled by Drop
}

#[test]
fn test_build_dependency_graph() {
    let notebook = TestNotebook::new("test_graph.rs", &create_multi_cell_notebook());
    let (graph, cell_ids) = notebook.build_graph();

    // Get execution order
    let order = graph
        .topological_order()
        .expect("Failed to get topological order");
    assert_eq!(order.len(), 3, "Expected 3 cells in order");

    let base_id = cell_ids["base"];
    let doubled_id = cell_ids["doubled"];
    let plus_ten_id = cell_ids["plus_ten"];

    // Verify all cells are in the order
    assert!(order.contains(&base_id), "Order should contain base");
    assert!(order.contains(&doubled_id), "Order should contain doubled");
    assert!(order.contains(&plus_ten_id), "Order should contain plus_ten");

    // Verify levels - base should be in first level, doubled in second, plus_ten in third
    let levels = graph.topological_levels(&order);
    assert_eq!(levels.len(), 3, "Expected 3 levels for linear chain");
}

#[test]
fn test_diamond_dependency_graph() {
    let notebook = TestNotebook::new("test_diamond.rs", &create_diamond_notebook());
    let (graph, cell_ids) = notebook.build_graph();

    let order = graph
        .topological_order()
        .expect("Failed to get topological order");
    assert_eq!(order.len(), 4, "Expected 4 cells");

    let root_id = cell_ids["root"];
    let left_id = cell_ids["left"];
    let right_id = cell_ids["right"];
    let merge_id = cell_ids["merge"];

    // Verify all cells are in the order
    assert!(order.contains(&root_id), "Order should contain root");
    assert!(order.contains(&left_id), "Order should contain left");
    assert!(order.contains(&right_id), "Order should contain right");
    assert!(order.contains(&merge_id), "Order should contain merge");

    // Verify levels - root in level 0, left/right in level 1, merge in level 2
    let levels = graph.topological_levels(&order);
    assert_eq!(levels.len(), 3, "Expected 3 levels for diamond");
}

#[test]
fn test_topological_levels() {
    let notebook = TestNotebook::new("test_levels.rs", &create_diamond_notebook());
    let (graph, _cell_ids) = notebook.build_graph();

    let order = graph
        .topological_order()
        .expect("Failed to get topological order");
    let levels = graph.topological_levels(&order);

    // Diamond should have 3 levels:
    // Level 0: root
    // Level 1: left, right (can execute in parallel)
    // Level 2: merge
    assert_eq!(levels.len(), 3, "Expected 3 levels");
    assert_eq!(levels[0].len(), 1, "Level 0 should have 1 cell (root)");
    assert_eq!(
        levels[1].len(),
        2,
        "Level 1 should have 2 cells (left, right)"
    );
    assert_eq!(levels[2].len(), 1, "Level 2 should have 1 cell (merge)");
}

#[test]
fn test_invalidation_propagation() {
    let notebook = TestNotebook::new("test_invalidation.rs", &create_multi_cell_notebook());
    let (graph, cell_ids) = notebook.build_graph();

    let base_id = cell_ids["base"];
    let doubled_id = cell_ids["doubled"];
    let plus_ten_id = cell_ids["plus_ten"];

    // Invalidating base should invalidate base, doubled, and plus_ten
    let invalidated = graph.invalidated_cells(base_id);
    assert!(
        invalidated.contains(&base_id),
        "base should be in invalidated set (the source)"
    );
    assert!(
        invalidated.contains(&doubled_id),
        "doubled should be invalidated"
    );
    assert!(
        invalidated.contains(&plus_ten_id),
        "plus_ten should be invalidated"
    );

    // Invalidating doubled should invalidate doubled and plus_ten, but NOT base
    let invalidated2 = graph.invalidated_cells(doubled_id);
    assert!(
        invalidated2.contains(&doubled_id),
        "doubled should be in invalidated set (the source)"
    );
    assert!(
        invalidated2.contains(&plus_ten_id),
        "plus_ten should be invalidated"
    );
    assert!(
        !invalidated2.contains(&base_id),
        "base should NOT be invalidated (it's upstream)"
    );
    assert_eq!(
        invalidated2.len(),
        2,
        "Should have exactly 2 invalidated cells (doubled, plus_ten)"
    );

    // Invalidating plus_ten should only invalidate plus_ten itself
    let invalidated3 = graph.invalidated_cells(plus_ten_id);
    assert_eq!(invalidated3.len(), 1, "plus_ten has no dependents");
    assert!(
        invalidated3.contains(&plus_ten_id),
        "plus_ten should be in its own invalidation set"
    );
}

#[test]
fn test_cycle_detection() {
    let cyclic_source = r#"
/// Cell A depends on C
#[venus::cell]
pub fn cell_a(cell_c: &i32) -> i32 {
    cell_c + 1
}

/// Cell B depends on A
#[venus::cell]
pub fn cell_b(cell_a: &i32) -> i32 {
    cell_a + 1
}

/// Cell C depends on B (creates cycle: A -> C -> B -> A)
#[venus::cell]
pub fn cell_c(cell_b: &i32) -> i32 {
    cell_b + 1
}
"#;

    let notebook = TestNotebook::new("test_cycle.rs", cyclic_source);
    let cells = notebook.parse();

    let mut graph = GraphEngine::new();
    for cell in &cells {
        graph.add_cell(cell.clone());
    }

    let result = graph.resolve_dependencies();
    assert!(result.is_err(), "Should detect cycle");

    let err = result.expect_err("resolve_dependencies should return an error for cyclic graph");
    let err_msg = err.to_string().to_lowercase();
    assert!(
        err_msg.contains("cycl"),
        "Error should mention cycle: {}",
        err_msg
    );
}

#[test]
fn test_state_manager_save_load() {
    let state_dir = TestStateDir::new("state");
    let mut state =
        StateManager::new(state_dir.path()).expect("Failed to create state manager");

    // Create a test cell ID
    let cell_id = CellId::new(1);

    // Save some output using BoxedOutput
    let output_data = vec![42u8, 0, 0, 0]; // i32 = 42 in little endian
    let boxed = BoxedOutput::from_raw_bytes(output_data.clone());
    state.store_output(cell_id, boxed);

    // Load it back from in-memory cache
    let loaded = state.get_output(cell_id);
    assert!(loaded.is_some(), "Output should be loaded from memory");
    assert_eq!(
        loaded.expect("Output should exist").bytes(),
        &output_data[..],
        "Data mismatch"
    );

    // Test persistence - flush to disk
    state.flush().expect("Failed to flush state to disk");
}

#[test]
fn test_state_invalidation() {
    let state_dir = TestStateDir::new("state_invalidation");
    let mut state =
        StateManager::new(state_dir.path()).expect("Failed to create state manager");

    let cell1 = CellId::new(1);
    let cell2 = CellId::new(2);
    let cell3 = CellId::new(3);

    // Save outputs for all cells using BoxedOutput
    state.store_output(cell1, BoxedOutput::from_raw_bytes(vec![1]));
    state.store_output(cell2, BoxedOutput::from_raw_bytes(vec![2]));
    state.store_output(cell3, BoxedOutput::from_raw_bytes(vec![3]));

    // Verify all exist
    assert!(state.get_output(cell1).is_some(), "cell1 should be stored");
    assert!(state.get_output(cell2).is_some(), "cell2 should be stored");
    assert!(state.get_output(cell3).is_some(), "cell3 should be stored");

    // Invalidate cell2 and cell3 (simulating dependent invalidation)
    state.invalidate(cell2);
    state.invalidate(cell3);

    // cell1 should still exist, cell2 and cell3 should be gone
    assert!(state.get_output(cell1).is_some(), "cell1 should remain");
    assert!(
        state.get_output(cell2).is_none(),
        "cell2 should be invalidated"
    );
    assert!(
        state.get_output(cell3).is_none(),
        "cell3 should be invalidated"
    );
}

#[test]
fn test_toolchain_detection() {
    // This test verifies toolchain manager can be created
    let toolchain = ToolchainManager::new().expect("Toolchain manager should initialize");

    // Should be able to get rustc path
    let rustc = toolchain.rustc_path();
    assert!(
        rustc.exists() || rustc.to_string_lossy().contains("rustc"),
        "Should have valid rustc path: {:?}",
        rustc
    );
}

#[test]
fn test_compiler_config() {
    let dev_config = CompilerConfig::development();
    assert!(dev_config.use_cranelift, "Dev config should use Cranelift");
    assert_eq!(dev_config.opt_level, 0, "Dev config should have opt_level 0");

    let prod_config = CompilerConfig::production();
    assert!(!prod_config.use_cranelift, "Prod config should use LLVM");
    assert_eq!(prod_config.opt_level, 3, "Prod config should have opt_level 3");
}

// =============================================================================
// Schema Evolution Tests
// =============================================================================

use venus_core::state::{SchemaChange, TypeFingerprint};

#[test]
fn test_schema_evolution_add_field() {
    // Simulate a struct evolving from v1 to v2 with an added field
    let v1 = TypeFingerprint::new(
        "Config",
        vec![
            ("name".to_string(), "String".to_string()),
            ("value".to_string(), "i32".to_string()),
        ],
    );

    let v2 = TypeFingerprint::new(
        "Config",
        vec![
            ("name".to_string(), "String".to_string()),
            ("value".to_string(), "i32".to_string()),
            ("enabled".to_string(), "bool".to_string()), // New field
        ],
    );

    let change = v1.compare(&v2);

    // Adding a field should be non-breaking (additive change)
    assert!(!change.is_breaking(), "Adding a field should not be breaking");
    match change {
        SchemaChange::Additive { added } => {
            assert_eq!(added.len(), 1, "Should have one added field");
            assert_eq!(added[0], "enabled", "Added field should be 'enabled'");
        }
        _ => panic!("Expected Additive change, got {:?}", change),
    }
}

#[test]
fn test_schema_evolution_remove_field() {
    // Simulate a struct evolving with a removed field
    let v1 = TypeFingerprint::new(
        "Config",
        vec![
            ("name".to_string(), "String".to_string()),
            ("value".to_string(), "i32".to_string()),
            ("deprecated".to_string(), "bool".to_string()),
        ],
    );

    let v2 = TypeFingerprint::new(
        "Config",
        vec![
            ("name".to_string(), "String".to_string()),
            ("value".to_string(), "i32".to_string()),
            // "deprecated" field removed
        ],
    );

    let change = v1.compare(&v2);

    // Removing a field should be breaking
    assert!(change.is_breaking(), "Removing a field should be breaking");
    match change {
        SchemaChange::Breaking { removed, .. } => {
            assert!(
                removed.contains(&"deprecated".to_string()),
                "Removed fields should contain 'deprecated'"
            );
        }
        _ => panic!("Expected Breaking change, got {:?}", change),
    }
}

#[test]
fn test_schema_evolution_with_state_manager() {
    let state_dir = TestStateDir::new("schema_evolution");
    let mut state = StateManager::new(state_dir.path()).expect("Failed to create state manager");

    let cell_id = CellId::new(1);

    // Store output with v1 schema fingerprint
    let v1_fingerprint = TypeFingerprint::new(
        "UserData",
        vec![
            ("id".to_string(), "u64".to_string()),
            ("name".to_string(), "String".to_string()),
        ],
    );
    let v1_data = vec![1u8, 2, 3, 4]; // Simulated serialized data
    let boxed = BoxedOutput::from_raw_with_type(
        v1_data.clone(),
        v1_fingerprint.structure_hash,
        v1_fingerprint.type_name.clone(),
    );
    state.store_output(cell_id, boxed);

    // Verify output is stored
    let output = state.get_output(cell_id);
    assert!(output.is_some(), "Output should be stored");

    // Simulate schema change (add field)
    let v2_fingerprint = TypeFingerprint::new(
        "UserData",
        vec![
            ("id".to_string(), "u64".to_string()),
            ("name".to_string(), "String".to_string()),
            ("email".to_string(), "Option<String>".to_string()), // New field
        ],
    );

    // Detect schema change
    let change = v1_fingerprint.compare(&v2_fingerprint);
    assert!(!change.is_breaking(), "Adding optional field should be non-breaking");

    // For breaking changes, we would invalidate:
    let v3_fingerprint = TypeFingerprint::new(
        "UserData",
        vec![
            ("id".to_string(), "String".to_string()), // Type change!
            ("name".to_string(), "String".to_string()),
        ],
    );

    let breaking_change = v1_fingerprint.compare(&v3_fingerprint);
    assert!(breaking_change.is_breaking(), "Type change should be breaking");

    // On breaking change, invalidate the cache
    if breaking_change.is_breaking() {
        state.invalidate(cell_id);
        assert!(
            state.get_output(cell_id).is_none(),
            "Output should be invalidated after breaking schema change"
        );
    }
}

// =============================================================================
// Parallel Execution Tests
// =============================================================================

#[test]
fn test_parallel_execution_correctness() {
    // This test verifies that parallel execution produces correct results
    // by simulating a diamond dependency pattern where cells can run in parallel

    let notebook = TestNotebook::new("parallel_test.rs", &create_diamond_notebook());
    let (graph, cell_ids) = notebook.build_graph();

    let order = graph
        .topological_order()
        .expect("Failed to get topological order");
    let levels = graph.topological_levels(&order);

    // Verify the structure is correct for parallel execution
    // Level 0: root (1 cell)
    // Level 1: left, right (2 cells - can run in parallel)
    // Level 2: merge (1 cell)
    assert_eq!(levels.len(), 3, "Should have 3 levels");
    assert_eq!(levels[0].len(), 1, "Level 0 should have 1 cell (root)");
    assert_eq!(levels[1].len(), 2, "Level 1 should have 2 cells (left, right)");
    assert_eq!(levels[2].len(), 1, "Level 2 should have 1 cell (merge)");

    // Verify that left and right are in level 1 (can execute in parallel)
    let left_id = cell_ids["left"];
    let right_id = cell_ids["right"];

    assert!(
        levels[1].contains(&left_id),
        "Level 1 should contain 'left' cell"
    );
    assert!(
        levels[1].contains(&right_id),
        "Level 1 should contain 'right' cell"
    );

    // Verify merge depends on both left and right
    let merge_id = cell_ids["merge"];
    let merge_deps = graph.get_cell(merge_id).map(|c| c.dependencies.clone());
    assert!(
        merge_deps.is_some(),
        "Merge cell should exist and have dependencies"
    );
}

#[test]
fn test_parallel_levels_independence() {
    // Test that cells within the same level have no dependencies on each other
    let notebook = TestNotebook::new("parallel_independence.rs", &create_diamond_notebook());
    let (graph, cell_ids) = notebook.build_graph();

    let order = graph
        .topological_order()
        .expect("Failed to get topological order");
    let levels = graph.topological_levels(&order);

    // For each level, verify no cell depends on another cell in the same level
    for (level_idx, level) in levels.iter().enumerate() {
        for &cell_id in level {
            let cell = graph.get_cell(cell_id).expect("Cell should exist");
            for dep in &cell.dependencies {
                // Find the producing cell for this dependency
                let producer_name = &dep.param_name;
                if let Some(&producer_id) = cell_ids.get(producer_name) {
                    // Producer should NOT be in the same level
                    assert!(
                        !level.contains(&producer_id),
                        "Cell '{}' in level {} depends on '{}' which is also in level {}. \
                         Cells in the same level should be independent.",
                        cell.name,
                        level_idx,
                        producer_name,
                        level_idx
                    );
                }
            }
        }
    }
}

#[test]
fn test_parallel_execution_state_isolation() {
    // Test that state is correctly isolated when cells execute in parallel
    let state_dir = TestStateDir::new("parallel_state");
    let mut state = StateManager::new(state_dir.path()).expect("Failed to create state manager");

    // Simulate parallel execution of independent cells
    let cell_a = CellId::new(1);
    let cell_b = CellId::new(2);
    let cell_c = CellId::new(3);

    // Store outputs (simulating parallel writes)
    state.store_output(cell_a, BoxedOutput::from_raw_bytes(vec![10]));
    state.store_output(cell_b, BoxedOutput::from_raw_bytes(vec![20]));
    state.store_output(cell_c, BoxedOutput::from_raw_bytes(vec![30]));

    // Verify each cell has its correct output (no cross-contamination)
    let output_a = state.get_output(cell_a).expect("cell_a output should exist");
    let output_b = state.get_output(cell_b).expect("cell_b output should exist");
    let output_c = state.get_output(cell_c).expect("cell_c output should exist");

    assert_eq!(output_a.bytes(), &[10], "cell_a should have value 10");
    assert_eq!(output_b.bytes(), &[20], "cell_b should have value 20");
    assert_eq!(output_c.bytes(), &[30], "cell_c should have value 30");

    // Verify stats
    let stats = state.stats();
    assert_eq!(
        stats.cached_outputs, 3,
        "Should have 3 cached outputs"
    );
}
