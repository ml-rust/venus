//! Type conversions for Salsa tracking.
//!
//! These types provide Salsa-compatible representations of core Venus types.
//! They implement the traits required by Salsa (Clone, PartialEq, Eq, Hash)
//! and provide bidirectional conversion with their source types.

use std::path::PathBuf;

use crate::graph::{CellId, CellInfo, Dependency, SourceSpan};

/// Serializable cell data for Salsa tracking.
///
/// This is a simplified version of [`CellInfo`] that implements
/// the traits required by Salsa (Clone, PartialEq, Eq, Hash).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellData {
    /// Cell index (assigned during parsing)
    pub id: usize,
    /// Function name
    pub name: String,
    /// Parameter names (dependency references)
    pub param_names: Vec<String>,
    /// Parameter types
    pub param_types: Vec<String>,
    /// Whether each parameter is a reference
    pub param_is_ref: Vec<bool>,
    /// Whether each parameter is mutable
    pub param_is_mut: Vec<bool>,
    /// Return type
    pub return_type: String,
    /// Documentation
    pub doc_comment: Option<String>,
    /// Source code
    pub source_code: String,
    /// Source file path
    pub source_file: PathBuf,
    /// Source location (start_line, start_col, end_line, end_col)
    pub span: (usize, usize, usize, usize),
}

impl From<CellInfo> for CellData {
    fn from(info: CellInfo) -> Self {
        Self {
            id: info.id.as_usize(),
            name: info.name,
            param_names: info
                .dependencies
                .iter()
                .map(|d| d.param_name.clone())
                .collect(),
            param_types: info
                .dependencies
                .iter()
                .map(|d| d.param_type.clone())
                .collect(),
            param_is_ref: info.dependencies.iter().map(|d| d.is_ref).collect(),
            param_is_mut: info.dependencies.iter().map(|d| d.is_mut).collect(),
            return_type: info.return_type,
            doc_comment: info.doc_comment,
            source_code: info.source_code,
            source_file: info.source_file,
            span: (
                info.span.start_line,
                info.span.start_col,
                info.span.end_line,
                info.span.end_col,
            ),
        }
    }
}

impl From<CellData> for CellInfo {
    fn from(data: CellData) -> Self {
        let dependencies: Vec<Dependency> = data
            .param_names
            .into_iter()
            .zip(data.param_types)
            .zip(data.param_is_ref)
            .zip(data.param_is_mut)
            .map(|(((name, ty), is_ref), is_mut)| Dependency {
                param_name: name,
                param_type: ty,
                is_ref,
                is_mut,
            })
            .collect();

        Self {
            id: CellId::new(data.id),
            name: data.name,
            dependencies,
            return_type: data.return_type,
            doc_comment: data.doc_comment,
            source_code: data.source_code,
            source_file: data.source_file,
            span: SourceSpan {
                start_line: data.span.0,
                start_col: data.span.1,
                end_line: data.span.2,
                end_col: data.span.3,
            },
        }
    }
}

/// Compiled cell data for Salsa tracking.
///
/// This is a Salsa-compatible version of [`crate::compile::CompiledCell`] that implements
/// the traits required by Salsa (Clone, PartialEq, Eq, Hash).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompiledCellData {
    /// Cell index
    pub cell_id: usize,

    /// Cell name
    pub name: String,

    /// Path to the compiled dynamic library
    pub dylib_path: PathBuf,

    /// Entry point symbol name
    pub entry_symbol: String,

    /// Hash of the cell source (for cache invalidation)
    pub source_hash: u64,

    /// Hash of dependencies (for cache invalidation)
    pub deps_hash: u64,

    /// Compilation time in milliseconds
    pub compile_time_ms: u64,
}

impl From<crate::compile::CompiledCell> for CompiledCellData {
    fn from(compiled: crate::compile::CompiledCell) -> Self {
        Self {
            cell_id: compiled.cell_id.as_usize(),
            name: compiled.name,
            dylib_path: compiled.dylib_path,
            entry_symbol: compiled.entry_symbol,
            source_hash: compiled.source_hash,
            deps_hash: compiled.deps_hash,
            compile_time_ms: compiled.compile_time_ms,
        }
    }
}

impl CompiledCellData {
    /// Convert back to [`crate::compile::CompiledCell`].
    pub fn to_compiled_cell(&self) -> crate::compile::CompiledCell {
        crate::compile::CompiledCell {
            cell_id: CellId::new(self.cell_id),
            name: self.name.clone(),
            dylib_path: self.dylib_path.clone(),
            entry_symbol: self.entry_symbol.clone(),
            source_hash: self.source_hash,
            deps_hash: self.deps_hash,
            compile_time_ms: self.compile_time_ms,
        }
    }
}

/// Compilation result wrapper for Salsa tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CompilationStatus {
    /// Compilation succeeded
    Success(CompiledCellData),
    /// Used cached result
    Cached(CompiledCellData),
    /// Compilation failed
    Failed(String),
}

impl CompilationStatus {
    /// Get the compiled cell data if successful.
    pub fn compiled(&self) -> Option<&CompiledCellData> {
        match self {
            Self::Success(data) | Self::Cached(data) => Some(data),
            Self::Failed(_) => None,
        }
    }

    /// Check if compilation was successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_) | Self::Cached(_))
    }
}

/// Cell execution output data for Salsa tracking.
///
/// This type stores the serialized output of a cell execution in a
/// Salsa-compatible format. The actual output bytes are stored along
/// with metadata for type checking and debugging.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CellOutputData {
    /// Cell index that produced this output
    pub cell_id: usize,

    /// Serialized output bytes (bincode format)
    pub bytes: Vec<u8>,

    /// Type hash for validation on deserialization
    pub type_hash: u64,

    /// Type name for debugging
    pub type_name: String,

    /// Hash of input values used to produce this output.
    /// Used to detect when inputs have changed and output is stale.
    pub inputs_hash: u64,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

impl CellOutputData {
    /// Create a new cell output from a BoxedOutput.
    pub fn from_boxed(
        cell_id: usize,
        boxed: &crate::state::BoxedOutput,
        inputs_hash: u64,
        execution_time_ms: u64,
    ) -> Self {
        Self {
            cell_id,
            bytes: boxed.bytes().to_vec(),
            type_hash: boxed.type_hash(),
            type_name: boxed.type_name().to_string(),
            inputs_hash,
            execution_time_ms,
        }
    }

    /// Convert to a BoxedOutput for deserialization.
    pub fn to_boxed(&self) -> crate::state::BoxedOutput {
        crate::state::BoxedOutput::from_raw_with_type(
            self.bytes.clone(),
            self.type_hash,
            self.type_name.clone(),
        )
    }

    /// Check if this output is valid for the given inputs hash.
    pub fn is_valid_for(&self, inputs_hash: u64) -> bool {
        self.inputs_hash == inputs_hash
    }
}

/// Execution status for a cell.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExecutionStatus {
    /// Cell has not been executed yet
    Pending,
    /// Cell is currently executing
    Running,
    /// Cell executed successfully
    Success(CellOutputData),
    /// Cell execution failed
    Failed(String),
}

impl ExecutionStatus {
    /// Get the output data if execution was successful.
    pub fn output(&self) -> Option<&CellOutputData> {
        match self {
            Self::Success(data) => Some(data),
            _ => None,
        }
    }

    /// Check if execution was successful.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    /// Check if execution failed.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// Combined graph analysis results for Salsa tracking.
///
/// This struct caches both execution order and parallel levels computed
/// from a single graph construction, eliminating redundant graph builds.
/// Both fields are computed together since they require the same GraphEngine.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphAnalysis {
    /// Topological execution order (cell indices)
    pub execution_order: Vec<usize>,

    /// Parallel execution levels (groups of cell indices that can run concurrently)
    pub parallel_levels: Vec<Vec<usize>>,
}

impl GraphAnalysis {
    /// Create an empty analysis (no cells or graph error).
    pub fn empty() -> Self {
        Self {
            execution_order: Vec::new(),
            parallel_levels: Vec::new(),
        }
    }

    /// Check if the analysis is empty (no cells to execute).
    pub fn is_empty(&self) -> bool {
        self.execution_order.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_data_conversion() {
        let info = CellInfo {
            id: CellId::new(0),
            name: "test".to_string(),
            dependencies: vec![
                Dependency {
                    param_name: "x".to_string(),
                    param_type: "i32".to_string(),
                    is_ref: true,
                    is_mut: false,
                },
                Dependency {
                    param_name: "y".to_string(),
                    param_type: "Vec<u8>".to_string(),
                    is_ref: true,
                    is_mut: true, // mutable reference
                },
            ],
            return_type: "i32".to_string(),
            doc_comment: Some("Test cell".to_string()),
            source_code: "{ 42 }".to_string(),
            source_file: PathBuf::from("test.rs"),
            span: SourceSpan {
                start_line: 1,
                start_col: 0,
                end_line: 1,
                end_col: 10,
            },
        };

        // Convert to CellData
        let data: CellData = info.clone().into();
        assert_eq!(data.name, "test");
        assert_eq!(data.param_names, vec!["x", "y"]);
        assert_eq!(data.param_types, vec!["i32", "Vec<u8>"]);
        assert_eq!(data.param_is_ref, vec![true, true]);
        assert_eq!(data.param_is_mut, vec![false, true]);

        // Convert back to CellInfo - verify no data loss
        let back: CellInfo = data.into();
        assert_eq!(back.name, "test");
        assert_eq!(back.dependencies.len(), 2);
        assert_eq!(back.dependencies[0].param_name, "x");
        assert!(back.dependencies[0].is_ref);
        assert!(!back.dependencies[0].is_mut);
        assert_eq!(back.dependencies[1].param_name, "y");
        assert!(back.dependencies[1].is_ref);
        assert!(back.dependencies[1].is_mut); // Preserved!
    }

    #[test]
    fn test_compiled_cell_data_conversion() {
        use crate::compile::CompiledCell;

        let compiled = CompiledCell {
            cell_id: CellId::new(0),
            name: "test_cell".to_string(),
            dylib_path: PathBuf::from("/path/to/cell.so"),
            entry_symbol: "venus_cell_test_cell".to_string(),
            source_hash: 12345,
            deps_hash: 67890,
            compile_time_ms: 100,
        };

        // Convert to CompiledCellData
        let data: CompiledCellData = compiled.clone().into();
        assert_eq!(data.cell_id, 0);
        assert_eq!(data.name, "test_cell");
        assert_eq!(data.dylib_path, PathBuf::from("/path/to/cell.so"));
        assert_eq!(data.entry_symbol, "venus_cell_test_cell");
        assert_eq!(data.source_hash, 12345);
        assert_eq!(data.deps_hash, 67890);

        // Convert back
        let back = data.to_compiled_cell();
        assert_eq!(back.cell_id.as_usize(), 0);
        assert_eq!(back.name, "test_cell");
    }

    #[test]
    fn test_compilation_status() {
        let data = CompiledCellData {
            cell_id: 0,
            name: "test".to_string(),
            dylib_path: PathBuf::from("/test.so"),
            entry_symbol: "venus_cell_test".to_string(),
            source_hash: 1,
            deps_hash: 2,
            compile_time_ms: 50,
        };

        let success = CompilationStatus::Success(data.clone());
        assert!(success.is_success());
        assert!(success.compiled().is_some());

        let cached = CompilationStatus::Cached(data);
        assert!(cached.is_success());
        assert!(cached.compiled().is_some());

        let failed = CompilationStatus::Failed("error".to_string());
        assert!(!failed.is_success());
        assert!(failed.compiled().is_none());
    }
}
