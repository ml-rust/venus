//! Salsa tracked query functions.
//!
//! These functions are memoized by Salsa. Results are cached and only
//! recomputed when their inputs change.
//!
//! # Error Handling
//!
//! Query functions that can fail return a [`QueryResult`] enum that captures
//! both the successful result and any errors. This allows callers to distinguish
//! between "no results" and "error occurred" cases.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::compile::DependencyParser;
use crate::graph::{CellId, CellInfo, CellParser, GraphEngine};

use super::conversions::{CellData, CompilationStatus};
use super::inputs::{CompilerSettings, SourceFile};

/// Result wrapper for Salsa queries that can fail.
///
/// # Why not `std::result::Result`?
///
/// Salsa's memoization requires return types to implement `Clone`, `PartialEq`,
/// `Eq`, and `Hash`. While `std::result::Result<T, E>` implements these traits
/// when `T` and `E` do, using a dedicated type provides:
///
/// 1. **Simpler error type**: Always `String`, avoiding generic error handling
/// 2. **Explicit intent**: Clearly marks Salsa-compatible error boundaries
/// 3. **Consistent API**: All Venus queries use the same error pattern
///
/// # Usage Pattern
///
/// Most queries come in pairs: a `*_result` variant that returns errors, and
/// a convenience variant that returns a default on failure:
///
/// ```ignore
/// // Option 1: Handle errors explicitly
/// match db.get_execution_order_result(source) {
///     QueryResult::Ok(order) => process(order),
///     QueryResult::Err(e) => log_error(e),
/// }
///
/// // Option 2: Use default on error (empty vec)
/// let order = db.get_execution_order(source); // Returns empty vec on error
/// ```
///
/// Use the `*_result` variant when you need to:
/// - Display error messages to users
/// - Distinguish "no cells found" from "parse error"
/// - Propagate errors to callers
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QueryResult<T> {
    /// Query succeeded with a result
    Ok(T),
    /// Query failed with an error message
    Err(String),
}

impl<T> QueryResult<T> {
    /// Returns true if the query succeeded.
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// Returns true if the query failed.
    pub fn is_err(&self) -> bool {
        matches!(self, Self::Err(_))
    }

    /// Get the result if successful.
    pub fn ok(&self) -> Option<&T> {
        match self {
            Self::Ok(v) => Some(v),
            Self::Err(_) => None,
        }
    }

    /// Get the error message if failed.
    pub fn err(&self) -> Option<&str> {
        match self {
            Self::Ok(_) => None,
            Self::Err(e) => Some(e),
        }
    }

    /// Unwrap the result, panicking if it's an error.
    ///
    /// # Panics
    ///
    /// Panics with the error message if the query failed. This is intentional
    /// for cases where failure indicates a programming error (e.g., in tests).
    /// For production code, prefer [`ok()`], [`unwrap_or()`], or pattern matching.
    pub fn unwrap(self) -> T {
        match self {
            Self::Ok(v) => v,
            Self::Err(e) => panic!("Query failed: {}", e),
        }
    }

    /// Unwrap the result or return a default value.
    ///
    /// This is the recommended way to handle errors when you have a sensible
    /// default (e.g., empty vector for missing data).
    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Self::Ok(v) => v,
            Self::Err(_) => default,
        }
    }
}

/// Tracked function: Parse cells from a source file with error reporting.
///
/// This query extracts all `#[venus::cell]` functions from the source.
/// Results are memoized and only recomputed when the source changes.
///
/// Unlike [`parse_cells`], this version reports parsing errors instead of
/// silently returning an empty vector.
#[salsa::tracked]
pub fn parse_cells_result(
    db: &dyn salsa::Database,
    source: SourceFile,
) -> QueryResult<Vec<CellData>> {
    let path = source.path(db);
    let text = source.text(db);

    let mut parser = CellParser::new();
    match parser.parse_str(&text, &path) {
        Ok(parse_result) => {
            QueryResult::Ok(parse_result.code_cells.into_iter().map(CellData::from).collect())
        }
        Err(e) => {
            let error_msg = format!("Failed to parse '{}': {}", path.display(), e);
            tracing::error!("{}", error_msg);
            QueryResult::Err(error_msg)
        }
    }
}

/// Tracked function: Parse cells from a source file.
///
/// This query extracts all `#[venus::cell]` functions from the source.
/// Results are memoized and only recomputed when the source changes.
///
/// Returns an empty vector on parse errors. Use [`parse_cells_result`] if you
/// need to distinguish between "no cells" and "parse error".
#[salsa::tracked]
pub fn parse_cells(db: &dyn salsa::Database, source: SourceFile) -> Vec<CellData> {
    parse_cells_result(db, source).unwrap_or(Vec::new())
}

/// Tracked function: Get cell names for quick lookup.
#[salsa::tracked]
pub fn cell_names(db: &dyn salsa::Database, source: SourceFile) -> Vec<String> {
    parse_cells(db, source)
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

/// Build a GraphEngine from parsed cells.
///
/// Takes ownership of cells to avoid cloning during conversion.
/// Returns an error message if dependency resolution fails.
fn build_graph_engine(cells: Vec<CellData>) -> Result<GraphEngine, String> {
    let mut engine = GraphEngine::new();

    for cell_data in cells {
        engine.add_cell(cell_data.into());
    }

    engine.resolve_dependencies().map_err(|e| {
        format!("Failed to resolve dependencies: {}", e)
    })?;

    Ok(engine)
}

/// Tracked function: Analyze dependency graph and compute execution metadata.
///
/// This is the central query for graph analysis. It builds the GraphEngine once
/// and computes both execution order and parallel levels together, eliminating
/// redundant graph construction. Other queries like `execution_order` and
/// `parallel_levels` extract their results from this cached analysis.
///
/// Returns a `QueryResult` with the analysis on success, or an error describing
/// what went wrong (parse errors, missing dependencies, cycles, etc.).
#[salsa::tracked]
pub fn graph_analysis_result(
    db: &dyn salsa::Database,
    source: SourceFile,
) -> QueryResult<super::conversions::GraphAnalysis> {
    // First check for parse errors
    let cells_result = parse_cells_result(db, source);
    let cells = match cells_result {
        QueryResult::Ok(c) => c,
        QueryResult::Err(e) => return QueryResult::Err(e),
    };

    if cells.is_empty() {
        return QueryResult::Ok(super::conversions::GraphAnalysis::empty());
    }

    // Build the graph once (takes ownership of cells)
    let engine = match build_graph_engine(cells) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("{}", e);
            return QueryResult::Err(e);
        }
    };

    // Get topological order
    let order = match engine.topological_order() {
        Ok(order) => order,
        Err(e) => {
            let error_msg = format!("Failed to compute execution order: {}", e);
            tracing::error!("{}", error_msg);
            return QueryResult::Err(error_msg);
        }
    };

    // Compute parallel levels from the same graph (no rebuild!)
    let parallel_levels = engine
        .topological_levels(&order)
        .into_iter()
        .map(|level| level.into_iter().map(|id| id.as_usize()).collect())
        .collect();

    let execution_order = order.into_iter().map(|id| id.as_usize()).collect();

    QueryResult::Ok(super::conversions::GraphAnalysis {
        execution_order,
        parallel_levels,
    })
}

/// Tracked function: Get cached graph analysis.
///
/// Returns the combined execution order and parallel levels.
/// Use [`graph_analysis_result`] if you need error details.
#[salsa::tracked]
pub fn graph_analysis(
    db: &dyn salsa::Database,
    source: SourceFile,
) -> super::conversions::GraphAnalysis {
    graph_analysis_result(db, source).unwrap_or(super::conversions::GraphAnalysis::empty())
}

/// Tracked function: Build and validate dependency graph with error reporting.
///
/// Returns the topological execution order if the graph is valid,
/// or an error describing what went wrong.
///
/// This query extracts execution order from the cached [`graph_analysis_result`].
#[salsa::tracked]
pub fn execution_order_result(
    db: &dyn salsa::Database,
    source: SourceFile,
) -> QueryResult<Vec<usize>> {
    match graph_analysis_result(db, source) {
        QueryResult::Ok(analysis) => QueryResult::Ok(analysis.execution_order),
        QueryResult::Err(e) => QueryResult::Err(e),
    }
}

/// Tracked function: Build and validate dependency graph.
///
/// Returns the topological execution order if the graph is valid,
/// or an empty vec if there are cycles or missing dependencies.
///
/// Use [`execution_order_result`] if you need to distinguish between
/// "no cells" and "graph error".
#[salsa::tracked]
pub fn execution_order(db: &dyn salsa::Database, source: SourceFile) -> Vec<usize> {
    graph_analysis(db, source).execution_order
}

/// Tracked function: Get cells invalidated by a change.
///
/// Note: This query still builds its own graph because it needs to call
/// `invalidated_cells()` which isn't part of the standard graph analysis.
#[salsa::tracked]
pub fn invalidated_by(
    db: &dyn salsa::Database,
    source: SourceFile,
    changed_idx: usize,
) -> Vec<usize> {
    let cells = parse_cells(db, source);

    let engine = match build_graph_engine(cells) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    engine
        .invalidated_cells(CellId::new(changed_idx))
        .into_iter()
        .map(|id| id.as_usize())
        .collect()
}

/// Tracked function: Get parallel execution levels.
///
/// Returns groups of cell indices that can be executed in parallel.
/// This query extracts parallel levels from the cached [`graph_analysis`].
#[salsa::tracked]
pub fn parallel_levels(db: &dyn salsa::Database, source: SourceFile) -> Vec<Vec<usize>> {
    graph_analysis(db, source).parallel_levels
}

/// Tracked function: Compute dependency hash from source.
///
/// This hash represents all external dependencies declared in the notebook.
/// Changes to dependencies will invalidate compiled cells.
#[salsa::tracked]
pub fn dependency_hash(db: &dyn salsa::Database, source: SourceFile) -> u64 {
    let text = source.text(db);

    let mut parser = DependencyParser::new();
    parser.parse(&text);

    let mut hasher = DefaultHasher::new();

    // Hash each dependency's name, version, features, and path
    for dep in parser.dependencies() {
        dep.name.hash(&mut hasher);
        dep.version.hash(&mut hasher);
        dep.features.hash(&mut hasher);
        if let Some(path) = &dep.path {
            path.hash(&mut hasher);
        }
    }

    hasher.finish()
}

/// Tracked function: Compile a cell.
///
/// This query compiles a cell to a dynamic library. Results are memoized
/// by Salsa, so repeated calls with the same inputs return cached results.
///
/// The compilation depends on:
/// - The cell's source code (via CellData from parse_cells)
/// - The dependency hash (via dependency_hash)
/// - The compiler settings (via CompilerSettings input)
#[salsa::tracked]
pub fn compiled_cell(
    db: &dyn salsa::Database,
    source: SourceFile,
    cell_idx: usize,
    settings: CompilerSettings,
) -> CompilationStatus {
    let cells = parse_cells(db, source);

    // Find the cell
    let Some(cell_data) = cells.get(cell_idx) else {
        return CompilationStatus::Failed(format!("Cell index {} not found", cell_idx));
    };

    // Get dependency hash
    let deps_hash = dependency_hash(db, source);

    // Convert to CellInfo for the compiler
    let cell_info: CellInfo = cell_data.clone().into();

    // Create compiler configuration
    let config = crate::compile::CompilerConfig {
        build_dir: settings.build_dir(db),
        cache_dir: settings.cache_dir(db),
        use_cranelift: settings.use_cranelift(db),
        debug_info: true,
        opt_level: settings.opt_level(db),
        extra_rustc_flags: Vec::new(),
        venus_crate_path: crate::compile::CompilerConfig::default().venus_crate_path,
    };

    // Create the compiler
    let toolchain = match crate::compile::ToolchainManager::new() {
        Ok(tc) => tc,
        Err(e) => {
            return CompilationStatus::Failed(format!("Toolchain error: {}", e));
        }
    };

    let mut compiler = crate::compile::CellCompiler::new(config, toolchain);

    // Set universe path if available
    if let Some(universe_path) = settings.universe_path(db) {
        compiler = compiler.with_universe(universe_path);
    }

    // Compile the cell
    match compiler.compile(&cell_info, deps_hash) {
        crate::compile::CompilationResult::Success(compiled) => {
            CompilationStatus::Success(compiled.into())
        }
        crate::compile::CompilationResult::Cached(compiled) => {
            CompilationStatus::Cached(compiled.into())
        }
        crate::compile::CompilationResult::Failed { errors, .. } => {
            let error_msg = errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
                .join("\n");
            CompilationStatus::Failed(error_msg)
        }
    }
}

/// Tracked function: Compile all cells in execution order.
///
/// Returns a list of compilation results for all cells, wrapped in `Arc` for
/// efficient sharing across clones of the query result without deep copying
/// the potentially large compilation results vector.
#[salsa::tracked]
pub fn compile_all_cells(
    db: &dyn salsa::Database,
    source: SourceFile,
    settings: CompilerSettings,
) -> Arc<Vec<CompilationStatus>> {
    let order = execution_order(db, source);

    let results: Vec<CompilationStatus> = order
        .iter()
        .map(|&idx| compiled_cell(db, source, idx, settings))
        .collect();

    Arc::new(results)
}

/// Tracked function: Get the execution status for a specific cell.
///
/// Returns the current execution status (pending, running, success, or failed)
/// for the specified cell. This query depends on the CellOutputs input, so
/// it will be recomputed when outputs are updated.
#[salsa::tracked]
pub fn cell_output(
    db: &dyn salsa::Database,
    outputs: super::inputs::CellOutputs,
    cell_idx: usize,
) -> super::conversions::ExecutionStatus {
    let statuses = outputs.statuses(db);
    statuses
        .get(cell_idx)
        .cloned()
        .unwrap_or(super::conversions::ExecutionStatus::Pending)
}

/// Tracked function: Check if all cells have completed execution.
///
/// Returns true if all cells have either succeeded or failed.
#[salsa::tracked]
pub fn all_cells_executed(
    db: &dyn salsa::Database,
    outputs: super::inputs::CellOutputs,
) -> bool {
    let statuses = outputs.statuses(db);
    statuses.iter().all(|s| {
        matches!(
            s,
            super::conversions::ExecutionStatus::Success(_)
                | super::conversions::ExecutionStatus::Failed(_)
        )
    })
}

/// Tracked function: Get successful output for a cell.
///
/// Returns the output data if the cell executed successfully,
/// or None if pending, running, or failed.
#[salsa::tracked]
pub fn cell_output_data(
    db: &dyn salsa::Database,
    outputs: super::inputs::CellOutputs,
    cell_idx: usize,
) -> Option<super::conversions::CellOutputData> {
    let status = cell_output(db, outputs, cell_idx);
    status.output().cloned()
}

#[cfg(test)]
mod tests {
    use crate::salsa_db::VenusDatabase;
    use std::path::PathBuf;

    #[test]
    fn test_parse_cells_query() {
        let db = VenusDatabase::new();

        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }

                #[venus::cell]
                pub fn b(a: &i32) -> i32 { *a + 1 }
            "#
            .to_string(),
        );

        let cells = db.get_cells(source);

        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].name, "a");
        assert_eq!(cells[1].name, "b");
    }

    #[test]
    fn test_execution_order_query() {
        let db = VenusDatabase::new();

        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }

                #[venus::cell]
                pub fn b(a: &i32) -> i32 { *a + 1 }
            "#
            .to_string(),
        );

        let order = db.get_execution_order(source);
        assert_eq!(order.len(), 2);
        // 'a' should come before 'b'
        assert_eq!(order[0], 0); // a
        assert_eq!(order[1], 1); // b
    }

    #[test]
    fn test_invalidated_cells_query() {
        let db = VenusDatabase::new();

        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }

                #[venus::cell]
                pub fn b(a: &i32) -> i32 { *a + 1 }

                #[venus::cell]
                pub fn c(b: &i32) -> i32 { *b + 1 }
            "#
            .to_string(),
        );

        // If 'a' changes, a, b, and c all need to re-execute
        // (the changed cell plus all its transitive dependents)
        let invalidated = db.get_invalidated(source, 0);
        assert_eq!(invalidated.len(), 3);
        assert_eq!(invalidated, vec![0, 1, 2]); // a -> b -> c in topological order
    }

    #[test]
    fn test_parallel_levels() {
        let db = VenusDatabase::new();

        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }

                #[venus::cell]
                pub fn b() -> i32 { 2 }

                #[venus::cell]
                pub fn c(a: &i32, b: &i32) -> i32 { *a + *b }
            "#
            .to_string(),
        );

        let levels = db.get_parallel_levels(source);
        assert_eq!(levels.len(), 2);
        // First level: a and b (can run in parallel)
        assert_eq!(levels[0].len(), 2);
        // Second level: c (depends on both)
        assert_eq!(levels[1].len(), 1);
    }

    #[test]
    fn test_dependency_hash() {
        let db = VenusDatabase::new();

        // Source with dependencies (using correct ```cargo block format)
        let source1 = db.set_source(
            PathBuf::from("test1.rs"),
            r#"
//! ```cargo
//! [dependencies]
//! tokio = "1"
//! serde = { version = "1.0", features = ["derive"] }
//! ```

#[venus::cell]
pub fn a() -> i32 { 1 }
            "#
            .to_string(),
        );

        // Same dependencies - should produce same hash
        let source2 = db.set_source(
            PathBuf::from("test2.rs"),
            r#"
//! ```cargo
//! [dependencies]
//! tokio = "1"
//! serde = { version = "1.0", features = ["derive"] }
//! ```

#[venus::cell]
pub fn b() -> i32 { 2 }
            "#
            .to_string(),
        );

        // Different dependencies - should produce different hash
        let source3 = db.set_source(
            PathBuf::from("test3.rs"),
            r#"
//! ```cargo
//! [dependencies]
//! anyhow = "1.0"
//! ```

#[venus::cell]
pub fn c() -> i32 { 3 }
            "#
            .to_string(),
        );

        let hash1 = db.get_dependency_hash(source1);
        let hash2 = db.get_dependency_hash(source2);
        let hash3 = db.get_dependency_hash(source3);

        // Same dependencies should have same hash
        assert_eq!(hash1, hash2);
        // Different dependencies should have different hash
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_query_result_methods() {
        use super::QueryResult;

        let ok: QueryResult<i32> = QueryResult::Ok(42);
        assert!(ok.is_ok());
        assert!(!ok.is_err());
        assert_eq!(ok.ok(), Some(&42));
        assert_eq!(ok.err(), None);
        assert_eq!(ok.unwrap(), 42);

        let err: QueryResult<i32> = QueryResult::Err("error".to_string());
        assert!(!err.is_ok());
        assert!(err.is_err());
        assert_eq!(err.ok(), None);
        assert_eq!(err.err(), Some("error"));
        assert_eq!(err.unwrap_or(0), 0);
    }

    #[test]
    fn test_parse_cells_result_success() {
        let db = VenusDatabase::new();

        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }
            "#
            .to_string(),
        );

        let result = db.get_cells_result(source);
        assert!(result.is_ok());
        assert_eq!(result.ok().unwrap().len(), 1);
    }

    #[test]
    fn test_execution_order_result_success() {
        let db = VenusDatabase::new();

        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }

                #[venus::cell]
                pub fn b(a: &i32) -> i32 { *a + 1 }
            "#
            .to_string(),
        );

        let result = db.get_execution_order_result(source);
        assert!(result.is_ok());
        assert_eq!(result.ok().unwrap().len(), 2);
    }
}
