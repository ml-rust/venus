//! Types for the graph engine.

use petgraph::graph::{DiGraph, NodeIndex};
use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::path::PathBuf;

use crate::error::{Error, Result};

/// Unique identifier for a cell within a notebook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CellId(pub(crate) usize);

impl CellId {
    pub fn new(id: usize) -> Self {
        Self(id)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

impl std::fmt::Display for CellId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cell_{}", self.0)
    }
}

/// Information about a cell's dependency on another cell.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Name of the parameter in the function signature
    pub param_name: String,
    /// Type of the parameter (as a string for display)
    pub param_type: String,
    /// Whether the parameter is a reference (&T or &mut T)
    pub is_ref: bool,
    /// Whether the parameter is mutable (&mut T)
    pub is_mut: bool,
}

/// Source span information for error reporting.
#[derive(Debug, Clone)]
pub struct SourceSpan {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

/// Type of cell in the notebook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellType {
    /// Code cell (executable Rust function with #[venus::cell]).
    Code,
    /// Markdown cell (pure text/documentation, non-executable).
    Markdown,
    /// Definition cell (imports, types, helper functions - non-executable but editable).
    Definition,
}

/// Complete information about a code cell.
#[derive(Debug, Clone)]
pub struct CellInfo {
    /// Unique identifier
    pub id: CellId,
    /// Function name (also serves as the output variable name)
    pub name: String,
    /// Human-readable display name (extracted from doc comment heading or defaults to function name)
    pub display_name: String,
    /// Dependencies (function parameters)
    pub dependencies: Vec<Dependency>,
    /// Return type (as a string for display)
    pub return_type: String,
    /// Documentation comments (markdown)
    pub doc_comment: Option<String>,
    /// Source code of the cell
    pub source_code: String,
    /// Location in source file
    pub span: SourceSpan,
    /// Source file path
    pub source_file: PathBuf,
}

/// Complete information about a markdown cell.
#[derive(Debug, Clone)]
pub struct MarkdownCell {
    /// Unique identifier
    pub id: CellId,
    /// Markdown content
    pub content: String,
    /// Location in source file
    pub span: SourceSpan,
    /// Source file path
    pub source_file: PathBuf,
    /// Whether this is the module-level doc comment (appears at the top)
    pub is_module_doc: bool,
}

/// Type of definition in a definition cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionType {
    /// Import statement (use ...)
    Import,
    /// Struct definition
    Struct,
    /// Enum definition
    Enum,
    /// Type alias
    TypeAlias,
    /// Impl block (trait implementations, methods)
    Impl,
    /// Helper function (fn without #[venus::cell])
    HelperFunction,
    /// Mixed definitions (multiple types in one block)
    Mixed,
}

/// Complete information about a definition cell.
#[derive(Debug, Clone)]
pub struct DefinitionCell {
    /// Unique identifier
    pub id: CellId,
    /// Definition content (source code)
    pub content: String,
    /// Type of definition
    pub definition_type: DefinitionType,
    /// Location in source file
    pub span: SourceSpan,
    /// Source file path
    pub source_file: PathBuf,
    /// Attached doc comments (stays WITH the definition)
    pub doc_comment: Option<String>,
}

/// The reactive dependency graph engine.
pub struct GraphEngine {
    /// The directed graph: edges go from producer to consumer
    graph: DiGraph<CellId, ()>,
    /// Cell ID to node index mapping
    node_indices: FxHashMap<CellId, NodeIndex>,
    /// Cell information by ID
    cells: FxHashMap<CellId, CellInfo>,
    /// Output name to producing cell mapping
    outputs: FxHashMap<String, CellId>,
    /// Definition cells by ID (imports, types, helpers)
    definition_cells: FxHashMap<CellId, DefinitionCell>,
    /// Next cell ID to assign
    next_id: usize,
}

impl GraphEngine {
    /// Create a new empty graph engine.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: FxHashMap::default(),
            cells: FxHashMap::default(),
            outputs: FxHashMap::default(),
            definition_cells: FxHashMap::default(),
            next_id: 0,
        }
    }

    /// Add a cell to the graph (first pass: collect cells).
    pub fn add_cell(&mut self, mut cell: CellInfo) -> CellId {
        let id = CellId::new(self.next_id);
        self.next_id += 1;
        cell.id = id;

        let node_idx = self.graph.add_node(id);
        self.node_indices.insert(id, node_idx);

        // Register the output (function name)
        self.outputs.insert(cell.name.clone(), id);
        self.cells.insert(id, cell);

        id
    }

    /// Resolve dependencies and build edges (second pass).
    pub fn resolve_dependencies(&mut self) -> Result<()> {
        // Collect all edges to add (can't mutate graph while iterating cells)
        let mut edges_to_add = Vec::new();

        for (cell_id, cell) in &self.cells {
            for dep in &cell.dependencies {
                // Find the cell that produces this dependency
                if let Some(&producer_id) = self.outputs.get(&dep.param_name) {
                    edges_to_add.push((producer_id, *cell_id));
                } else {
                    // Dependency not found - this is an error
                    return Err(Error::CellNotFound(format!(
                        "Cell '{}' depends on '{}', but no cell produces it",
                        cell.name, dep.param_name
                    )));
                }
            }
        }

        // Add all edges
        for (producer, consumer) in edges_to_add {
            let producer_idx = self.node_indices[&producer];
            let consumer_idx = self.node_indices[&consumer];
            self.graph.add_edge(producer_idx, consumer_idx, ());
        }

        // Check for cycles
        self.detect_cycles()?;

        Ok(())
    }

    /// Detect cycles in the graph and return a helpful error message.
    fn detect_cycles(&self) -> Result<()> {
        use petgraph::algo::kosaraju_scc;

        let sccs = kosaraju_scc(&self.graph);

        for scc in sccs {
            if scc.len() > 1 {
                // Found a cycle - build a helpful error message
                let cycle_names: Vec<String> = scc
                    .iter()
                    .map(|&idx| {
                        let cell_id = self.graph[idx];
                        self.cells[&cell_id].name.clone()
                    })
                    .collect();

                return Err(Error::CyclicDependency(format!(
                    "Cyclic dependency detected: {} → {}",
                    cycle_names.join(" → "),
                    cycle_names[0]
                )));
            }
        }

        Ok(())
    }

    /// Get cells in topological order (respecting dependencies).
    pub fn topological_order(&self) -> Result<Vec<CellId>> {
        use petgraph::algo::toposort;

        toposort(&self.graph, None)
            .map(|nodes| nodes.into_iter().map(|idx| self.graph[idx]).collect())
            .map_err(|cycle| {
                let cell_id = self.graph[cycle.node_id()];
                let cell_name = &self.cells[&cell_id].name;
                Error::CyclicDependency(format!("Cycle detected at cell '{}'", cell_name))
            })
    }

    /// Get cells that need re-execution when `changed` is modified.
    ///
    /// Returns the changed cell plus all its transitive dependents,
    /// in topological order.
    pub fn invalidated_cells(&self, changed: CellId) -> Vec<CellId> {
        let mut invalidated = vec![changed];
        let mut queue = VecDeque::from([changed]);

        // BFS to find all dependents
        while let Some(cell_id) = queue.pop_front() {
            if let Some(&node_idx) = self.node_indices.get(&cell_id) {
                for neighbor_idx in self.graph.neighbors(node_idx) {
                    let neighbor_id = self.graph[neighbor_idx];
                    if !invalidated.contains(&neighbor_id) {
                        invalidated.push(neighbor_id);
                        queue.push_back(neighbor_id);
                    }
                }
            }
        }

        // Return in topological order
        self.topological_order()
            .unwrap_or_default()
            .into_iter()
            .filter(|c| invalidated.contains(c))
            .collect()
    }

    /// Group cells by dependency level for parallel execution.
    ///
    /// Cells in the same level have no dependencies on each other
    /// and can be executed in parallel.
    pub fn topological_levels(&self, cells: &[CellId]) -> Vec<Vec<CellId>> {
        let cell_set: std::collections::HashSet<_> = cells.iter().copied().collect();
        let mut levels = Vec::new();
        let mut remaining: std::collections::HashSet<_> = cells.iter().copied().collect();
        let mut completed = std::collections::HashSet::new();

        while !remaining.is_empty() {
            // Find cells whose dependencies are all completed
            let ready: Vec<CellId> = remaining
                .iter()
                .copied()
                .filter(|&cell_id| {
                    let cell = &self.cells[&cell_id];
                    cell.dependencies.iter().all(|dep| {
                        // Dependency is satisfied if:
                        // 1. It's not in our cell set (external), or
                        // 2. It's already completed
                        self.outputs
                            .get(&dep.param_name)
                            .map(|&producer_id| {
                                !cell_set.contains(&producer_id) || completed.contains(&producer_id)
                            })
                            .unwrap_or(true)
                    })
                })
                .collect();

            if ready.is_empty() && !remaining.is_empty() {
                // This shouldn't happen if cycles are already detected
                break;
            }

            for cell_id in &ready {
                remaining.remove(cell_id);
                completed.insert(*cell_id);
            }

            if !ready.is_empty() {
                levels.push(ready);
            }
        }

        levels
    }

    /// Get a cell by ID.
    pub fn get_cell(&self, id: CellId) -> Option<&CellInfo> {
        self.cells.get(&id)
    }

    /// Get a cell by name.
    pub fn get_cell_by_name(&self, name: &str) -> Option<&CellInfo> {
        self.outputs.get(name).and_then(|id| self.cells.get(id))
    }

    /// Get all cells.
    pub fn cells(&self) -> impl Iterator<Item = &CellInfo> {
        self.cells.values()
    }

    /// Get the number of cells.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Check if the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Get direct dependencies of a cell.
    pub fn dependencies(&self, id: CellId) -> Vec<CellId> {
        self.cells
            .get(&id)
            .map(|cell| {
                cell.dependencies
                    .iter()
                    .filter_map(|dep| self.outputs.get(&dep.param_name).copied())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get direct dependents of a cell (cells that depend on this one).
    pub fn dependents(&self, id: CellId) -> Vec<CellId> {
        self.node_indices
            .get(&id)
            .map(|&idx| {
                self.graph
                    .neighbors(idx)
                    .map(|neighbor_idx| self.graph[neighbor_idx])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Add a definition cell to the graph.
    pub fn add_definition_cell(&mut self, mut cell: DefinitionCell) -> CellId {
        let id = CellId::new(self.next_id);
        self.next_id += 1;
        cell.id = id;
        self.definition_cells.insert(id, cell);
        id
    }

    /// Get a definition cell by ID.
    pub fn get_definition_cell(&self, id: CellId) -> Option<&DefinitionCell> {
        self.definition_cells.get(&id)
    }

    /// Get a mutable reference to a definition cell by ID.
    pub fn get_definition_cell_mut(&mut self, id: CellId) -> Option<&mut DefinitionCell> {
        self.definition_cells.get_mut(&id)
    }

    /// Get all definition cells.
    pub fn definition_cells(&self) -> impl Iterator<Item = &DefinitionCell> {
        self.definition_cells.values()
    }

    /// Remove a definition cell by ID.
    pub fn remove_definition_cell(&mut self, id: CellId) -> Option<DefinitionCell> {
        self.definition_cells.remove(&id)
    }
}

impl Default for GraphEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cell(name: &str, deps: &[&str]) -> CellInfo {
        CellInfo {
            id: CellId::new(0), // Will be assigned by add_cell
            name: name.to_string(),
            display_name: name.to_string(), // Default to function name
            dependencies: deps
                .iter()
                .map(|&d| Dependency {
                    param_name: d.to_string(),
                    param_type: "()".to_string(),
                    is_ref: true,
                    is_mut: false,
                })
                .collect(),
            return_type: "()".to_string(),
            doc_comment: None,
            source_code: String::new(),
            span: SourceSpan {
                start_line: 0,
                start_col: 0,
                end_line: 0,
                end_col: 0,
            },
            source_file: PathBuf::new(),
        }
    }

    #[test]
    fn test_empty_graph() {
        let graph = GraphEngine::new();
        assert!(graph.is_empty());
        assert_eq!(graph.len(), 0);
    }

    #[test]
    fn test_add_cell() {
        let mut graph = GraphEngine::new();
        let id = graph.add_cell(make_cell("foo", &[]));
        assert_eq!(graph.len(), 1);
        assert!(graph.get_cell(id).is_some());
        assert!(graph.get_cell_by_name("foo").is_some());
    }

    #[test]
    fn test_linear_dependencies() {
        let mut graph = GraphEngine::new();
        graph.add_cell(make_cell("a", &[]));
        graph.add_cell(make_cell("b", &["a"]));
        graph.add_cell(make_cell("c", &["b"]));
        graph.resolve_dependencies().unwrap();

        let order = graph.topological_order().unwrap();
        let names: Vec<_> = order
            .iter()
            .map(|id| graph.get_cell(*id).unwrap().name.clone())
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_diamond_dependencies() {
        let mut graph = GraphEngine::new();
        graph.add_cell(make_cell("a", &[]));
        graph.add_cell(make_cell("b", &["a"]));
        graph.add_cell(make_cell("c", &["a"]));
        graph.add_cell(make_cell("d", &["b", "c"]));
        graph.resolve_dependencies().unwrap();

        let order = graph.topological_order().unwrap();
        let names: Vec<_> = order
            .iter()
            .map(|id| graph.get_cell(*id).unwrap().name.clone())
            .collect();

        // a must come first, d must come last
        assert_eq!(names[0], "a");
        assert_eq!(names[3], "d");
    }

    #[test]
    fn test_cycle_detection() {
        let mut graph = GraphEngine::new();
        graph.add_cell(make_cell("a", &["c"]));
        graph.add_cell(make_cell("b", &["a"]));
        graph.add_cell(make_cell("c", &["b"]));

        let result = graph.resolve_dependencies();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::CyclicDependency(_)));
    }

    #[test]
    fn test_invalidated_cells() {
        let mut graph = GraphEngine::new();
        let a = graph.add_cell(make_cell("a", &[]));
        let b = graph.add_cell(make_cell("b", &["a"]));
        let c = graph.add_cell(make_cell("c", &["b"]));
        graph.add_cell(make_cell("d", &[])); // Independent
        graph.resolve_dependencies().unwrap();

        let invalidated = graph.invalidated_cells(a);
        assert!(invalidated.contains(&a));
        assert!(invalidated.contains(&b));
        assert!(invalidated.contains(&c));
        // d should not be invalidated
        assert!(
            !invalidated
                .iter()
                .any(|id| graph.get_cell(*id).unwrap().name == "d")
        );
    }

    #[test]
    fn test_topological_levels() {
        let mut graph = GraphEngine::new();
        let a = graph.add_cell(make_cell("a", &[]));
        let b = graph.add_cell(make_cell("b", &[]));
        let c = graph.add_cell(make_cell("c", &["a"]));
        let d = graph.add_cell(make_cell("d", &["b"]));
        let e = graph.add_cell(make_cell("e", &["c", "d"]));
        graph.resolve_dependencies().unwrap();

        let levels = graph.topological_levels(&[a, b, c, d, e]);
        assert_eq!(levels.len(), 3);

        // Level 0: a and b (no deps)
        assert!(levels[0].contains(&a));
        assert!(levels[0].contains(&b));

        // Level 1: c and d
        assert!(levels[1].contains(&c));
        assert!(levels[1].contains(&d));

        // Level 2: e
        assert!(levels[2].contains(&e));
    }

    #[test]
    fn test_missing_dependency() {
        let mut graph = GraphEngine::new();
        graph.add_cell(make_cell("a", &["nonexistent"]));

        let result = graph.resolve_dependencies();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::CellNotFound(_)));
    }
}
