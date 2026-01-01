//! Salsa-based incremental computation database for Venus.
//!
//! This module provides memoized queries for:
//! - Source file parsing
//! - Cell extraction
//! - Dependency graph construction
//! - Cell compilation (with caching)
//!
//! Changes to inputs automatically invalidate dependent queries,
//! enabling efficient incremental recomputation.
//!
//! # Module Organization
//!
//! - [`inputs`] - Input types (SourceFile, CompilerSettings)
//! - [`queries`] - Tracked query functions
//! - [`conversions`] - Type conversions for Salsa compatibility
//! - [`cache`] - Disk persistence for compilation cache

pub mod cache;
mod conversions;
mod inputs;
mod queries;

use std::path::PathBuf;
use std::sync::Arc;

use salsa::Setter;

// Re-export public types
pub use conversions::{
    CellData, CellOutputData, CompilationStatus, CompiledCellData, ExecutionStatus, GraphAnalysis,
};
pub use inputs::{CellOutputs, CompilerSettings, SourceFile};
pub use queries::{
    all_cells_executed, cell_names, cell_output, cell_output_data, compile_all_cells, compiled_cell,
    dependency_hash, execution_order, execution_order_result, graph_analysis, graph_analysis_result,
    invalidated_by, parallel_levels, parse_cells, parse_cells_result, QueryResult,
};

/// The concrete database implementation.
///
/// This is the main entry point for incremental computation in Venus.
/// Create an instance with [`VenusDatabase::new()`] and use the helper
/// methods to interact with the incremental system.
#[salsa::db]
#[derive(Default, Clone)]
pub struct VenusDatabase {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for VenusDatabase {}

impl VenusDatabase {
    /// Create a new Venus database.
    pub fn new() -> Self {
        Self::default()
    }

    // =========================================================================
    // Source File Management
    // =========================================================================

    /// Create a new source file input.
    ///
    /// Returns a handle that can be used with query functions.
    pub fn set_source(&self, path: PathBuf, text: String) -> SourceFile {
        SourceFile::new(self, path, text)
    }

    /// Update an existing source file's content.
    ///
    /// This will invalidate all queries that depend on this source.
    pub fn update_source(&mut self, source: SourceFile, text: String) {
        source.set_text(self).to(text);
    }

    // =========================================================================
    // Cell Queries
    // =========================================================================

    /// Parse cells from a source file.
    ///
    /// Returns the list of cells extracted from the source.
    /// Returns an empty vector on parse errors.
    pub fn get_cells(&self, source: SourceFile) -> Vec<CellData> {
        parse_cells(self, source)
    }

    /// Parse cells from a source file with error reporting.
    ///
    /// Returns `QueryResult::Ok` with cells on success, or `QueryResult::Err`
    /// with an error message on parse failure.
    pub fn get_cells_result(&self, source: SourceFile) -> QueryResult<Vec<CellData>> {
        parse_cells_result(self, source)
    }

    /// Get cell names from a source file.
    pub fn get_cell_names(&self, source: SourceFile) -> Vec<String> {
        cell_names(self, source)
    }

    /// Get the execution order for a notebook.
    ///
    /// Returns cell indices in topological order.
    /// Returns an empty vector on graph errors.
    pub fn get_execution_order(&self, source: SourceFile) -> Vec<usize> {
        execution_order(self, source)
    }

    /// Get the execution order for a notebook with error reporting.
    ///
    /// Returns `QueryResult::Ok` with ordered indices on success, or
    /// `QueryResult::Err` with an error message on graph errors (cycles,
    /// missing dependencies, etc.).
    pub fn get_execution_order_result(&self, source: SourceFile) -> QueryResult<Vec<usize>> {
        execution_order_result(self, source)
    }

    /// Get cells invalidated by a change.
    ///
    /// Returns all cells that need to be re-executed when the given cell changes.
    pub fn get_invalidated(&self, source: SourceFile, changed_idx: usize) -> Vec<usize> {
        invalidated_by(self, source, changed_idx)
    }

    /// Get parallel execution levels.
    ///
    /// Returns groups of cells that can be executed in parallel.
    pub fn get_parallel_levels(&self, source: SourceFile) -> Vec<Vec<usize>> {
        parallel_levels(self, source)
    }

    // =========================================================================
    // Compilation
    // =========================================================================

    /// Create compiler settings input.
    pub fn create_compiler_settings(
        &self,
        build_dir: PathBuf,
        cache_dir: PathBuf,
        universe_path: Option<PathBuf>,
        use_cranelift: bool,
        opt_level: u8,
    ) -> CompilerSettings {
        CompilerSettings::new(
            self,
            build_dir,
            cache_dir,
            universe_path,
            use_cranelift,
            opt_level,
        )
    }

    /// Get the dependency hash for a notebook.
    ///
    /// This hash represents all external crate dependencies.
    pub fn get_dependency_hash(&self, source: SourceFile) -> u64 {
        dependency_hash(self, source)
    }

    /// Compile a specific cell.
    ///
    /// Returns the compilation status (success, cached, or failed).
    pub fn compile_cell(
        &self,
        source: SourceFile,
        cell_idx: usize,
        settings: CompilerSettings,
    ) -> CompilationStatus {
        compiled_cell(self, source, cell_idx, settings)
    }

    /// Compile all cells in execution order.
    ///
    /// Returns compilation results for all cells.
    pub fn compile_all(
        &self,
        source: SourceFile,
        settings: CompilerSettings,
    ) -> Arc<Vec<CompilationStatus>> {
        compile_all_cells(self, source, settings)
    }

    // =========================================================================
    // Cell Outputs
    // =========================================================================

    /// Create a new cell outputs input with all cells pending.
    ///
    /// Call this after parsing cells to initialize the outputs tracking.
    pub fn create_cell_outputs(&self, cell_count: usize) -> CellOutputs {
        CellOutputs::new(
            self,
            Arc::new(vec![ExecutionStatus::Pending; cell_count]),
            0,
        )
    }

    /// Update the execution status for a specific cell.
    ///
    /// This will increment the version counter and invalidate any
    /// queries that depend on this cell's output.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `cell_idx` is out of bounds. In release
    /// builds, out-of-bounds indices are silently ignored (but version is
    /// still incremented, which may cause unnecessary invalidations).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let outputs = db.create_cell_outputs(3);
    /// db.set_cell_output(outputs, 0, ExecutionStatus::Running);
    /// ```
    pub fn set_cell_output(
        &mut self,
        outputs: CellOutputs,
        cell_idx: usize,
        status: ExecutionStatus,
    ) {
        let mut statuses = (*outputs.statuses(self)).clone();

        // Debug assertion to catch programming errors early
        debug_assert!(
            cell_idx < statuses.len(),
            "cell_idx {} is out of bounds (len={}). \
             Did you forget to call create_cell_outputs() with the correct count?",
            cell_idx,
            statuses.len()
        );

        if cell_idx < statuses.len() {
            statuses[cell_idx] = status;
            let new_version = outputs.version(self) + 1;
            outputs.set_statuses(self).to(Arc::new(statuses));
            outputs.set_version(self).to(new_version);
        } else {
            // Log warning in release builds for diagnosability
            tracing::warn!(
                "Attempted to set output for cell {} but only {} cells exist",
                cell_idx,
                statuses.len()
            );
        }
    }

    /// Get the execution status for a specific cell.
    ///
    /// Returns `ExecutionStatus::Pending` if the cell index is out of bounds.
    pub fn get_cell_output(&self, outputs: CellOutputs, cell_idx: usize) -> ExecutionStatus {
        cell_output(self, outputs, cell_idx)
    }

    /// Get the output data for a cell if it executed successfully.
    ///
    /// Returns `None` if the cell is pending, running, failed, or out of bounds.
    pub fn get_cell_output_data(&self, outputs: CellOutputs, cell_idx: usize) -> Option<CellOutputData> {
        cell_output_data(self, outputs, cell_idx)
    }

    /// Check if all cells have finished executing.
    pub fn are_all_cells_executed(&self, outputs: CellOutputs) -> bool {
        all_cells_executed(self, outputs)
    }

    /// Mark a cell as currently running.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `cell_idx` is out of bounds.
    pub fn mark_cell_running(&mut self, outputs: CellOutputs, cell_idx: usize) {
        self.set_cell_output(outputs, cell_idx, ExecutionStatus::Running);
    }

    /// Mark a cell as failed with an error message.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `cell_idx` is out of bounds.
    pub fn mark_cell_failed(&mut self, outputs: CellOutputs, cell_idx: usize, error: String) {
        self.set_cell_output(outputs, cell_idx, ExecutionStatus::Failed(error));
    }

    /// Mark a cell as successfully executed with output data.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `cell_idx` is out of bounds.
    pub fn mark_cell_success(&mut self, outputs: CellOutputs, cell_idx: usize, output: CellOutputData) {
        self.set_cell_output(outputs, cell_idx, ExecutionStatus::Success(output));
    }

    // =========================================================================
    // Cache Persistence
    // =========================================================================

    /// Create a cache snapshot from current compilation state.
    ///
    /// This captures all successfully compiled cells so they can be
    /// restored on the next startup without recompilation.
    ///
    /// # Arguments
    ///
    /// * `toolchain_version` - Current rustc version string
    /// * `dependency_hash` - Hash of external dependencies
    /// * `cells` - List of (name, source_hash, compilation_status)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let snapshot = db.create_cache_snapshot(
    ///     toolchain.version().to_string(),
    ///     db.get_dependency_hash(source),
    ///     compiled_cells,
    /// );
    /// CachePersistence::save(&cache_path, &snapshot)?;
    /// ```
    pub fn create_cache_snapshot(
        &self,
        toolchain_version: String,
        dependency_hash: u64,
        cells: Vec<(String, u64, CompilationStatus)>,
    ) -> cache::CacheSnapshot {
        let mut snapshot = cache::CacheSnapshot::new(toolchain_version, dependency_hash);

        for (name, source_hash, status) in cells {
            let cached_cell = match status {
                CompilationStatus::Success(ref data) => cache::CachedCell::success(
                    name,
                    source_hash,
                    data.dylib_path.to_string_lossy().to_string(),
                ),
                CompilationStatus::Cached(ref data) => cache::CachedCell::cached(
                    name,
                    source_hash,
                    data.dylib_path.to_string_lossy().to_string(),
                ),
                CompilationStatus::Failed(ref error) => {
                    cache::CachedCell::failed(name, source_hash, error.clone())
                }
            };
            snapshot.add_cell(cached_cell);
        }

        snapshot
    }

    /// Check if a cached cell can be reused.
    ///
    /// Returns `true` if the cell exists in the cache with a matching
    /// source hash and successful compilation status.
    pub fn is_cell_cached(
        &self,
        snapshot: &cache::CacheSnapshot,
        cell_name: &str,
        current_source_hash: u64,
    ) -> bool {
        snapshot
            .get_cell(cell_name)
            .map(|c| c.source_hash == current_source_hash && c.is_success())
            .unwrap_or(false)
    }

    /// Get the dylib path for a cached cell.
    ///
    /// Returns `None` if the cell is not in cache or failed compilation.
    pub fn get_cached_dylib_path(
        &self,
        snapshot: &cache::CacheSnapshot,
        cell_name: &str,
    ) -> Option<PathBuf> {
        snapshot.get_cell(cell_name).and_then(|c| {
            if c.is_success() && !c.dylib_path.is_empty() {
                Some(PathBuf::from(&c.dylib_path))
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_creation() {
        let _db = VenusDatabase::new();
    }

    #[test]
    fn test_source_file_input() {
        let db = VenusDatabase::new();

        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn config() -> i32 { 42 }
            "#
            .to_string(),
        );

        assert_eq!(source.path(&db), PathBuf::from("test.rs"));
    }

    #[test]
    fn test_incremental_update() {
        let mut db = VenusDatabase::new();

        // Initial source
        let source = db.set_source(
            PathBuf::from("test.rs"),
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }
            "#
            .to_string(),
        );

        let order1 = db.get_execution_order(source);
        assert_eq!(order1.len(), 1);

        // Update source - add a new cell
        db.update_source(
            source,
            r#"
                #[venus::cell]
                pub fn a() -> i32 { 1 }

                #[venus::cell]
                pub fn b(a: &i32) -> i32 { *a + 1 }
            "#
            .to_string(),
        );

        // Salsa automatically invalidates and recomputes
        let order2 = db.get_execution_order(source);
        assert_eq!(order2.len(), 2);
    }

    #[test]
    fn test_compiler_settings() {
        let db = VenusDatabase::new();

        let settings = db.create_compiler_settings(
            PathBuf::from(".venus/build"),
            PathBuf::from(".venus/cache"),
            Some(PathBuf::from(".venus/universe/libvenus_universe.so")),
            true,
            0,
        );

        assert_eq!(settings.build_dir(&db), PathBuf::from(".venus/build"));
        assert_eq!(settings.cache_dir(&db), PathBuf::from(".venus/cache"));
        assert!(settings.use_cranelift(&db));
        assert_eq!(settings.opt_level(&db), 0);
    }

    #[test]
    fn test_cell_outputs_creation() {
        let db = VenusDatabase::new();

        let outputs = db.create_cell_outputs(3);

        // All cells should start as pending
        assert!(matches!(
            db.get_cell_output(outputs, 0),
            ExecutionStatus::Pending
        ));
        assert!(matches!(
            db.get_cell_output(outputs, 1),
            ExecutionStatus::Pending
        ));
        assert!(matches!(
            db.get_cell_output(outputs, 2),
            ExecutionStatus::Pending
        ));

        // Not all cells are executed yet
        assert!(!db.are_all_cells_executed(outputs));
    }

    #[test]
    fn test_cell_output_updates() {
        let mut db = VenusDatabase::new();

        let outputs = db.create_cell_outputs(2);

        // Mark cell 0 as running
        db.mark_cell_running(outputs, 0);
        assert!(matches!(
            db.get_cell_output(outputs, 0),
            ExecutionStatus::Running
        ));

        // Mark cell 0 as failed
        db.mark_cell_failed(outputs, 0, "error message".to_string());
        assert!(matches!(
            db.get_cell_output(outputs, 0),
            ExecutionStatus::Failed(_)
        ));

        // Mark cell 1 as successful with output data
        let output_data = CellOutputData {
            cell_id: 1,
            bytes: vec![1, 2, 3],
            type_hash: 12345,
            type_name: "i32".to_string(),
            inputs_hash: 67890,
            execution_time_ms: 100,
        };
        db.mark_cell_success(outputs, 1, output_data.clone());

        // Get output data
        let retrieved = db.get_cell_output_data(outputs, 1);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().cell_id, 1);

        // All cells are now executed (one failed, one succeeded)
        assert!(db.are_all_cells_executed(outputs));
    }
}
