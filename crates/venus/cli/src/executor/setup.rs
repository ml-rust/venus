use super::NotebookExecutor;
use std::collections::HashMap;
use std::{fs, path::Path};
use venus_core::compile::{CompilerConfig, ToolchainManager, UniverseBuilder};
use venus_core::graph::{CellId, CellParser, GraphEngine};
use venus_core::paths::NotebookDirs;

impl NotebookExecutor {
    /// Create a new notebook executor.
    ///
    /// This performs the setup phase: parsing, dependency resolution, and universe building.
    pub fn new(notebook_path: &str, release: bool) -> anyhow::Result<Self> {
        let path = Path::new(notebook_path);
        if !path.exists() {
            anyhow::bail!("Notebook not found: {}", notebook_path);
        }

        let source = fs::read_to_string(path)?;
        let abs_path = path.canonicalize()?;
        let dirs = NotebookDirs::from_notebook_path(&abs_path)?;

        // Initialize toolchain
        Self::print_step("Checking toolchain");
        let toolchain = ToolchainManager::new()?;
        Self::print_success(None);

        // Parse cells
        Self::print_step("Parsing cells");
        let mut parser = CellParser::new();
        let parse_result = parser.parse_str(&source, &abs_path)?;
        let cells = parse_result.code_cells;
        let definition_cells = parse_result.definition_cells;
        Self::print_success(Some(&format!("{} code cells", cells.len())));

        // Build dependency graph
        Self::print_step("Building dependency graph");
        let mut graph = GraphEngine::new();
        let mut cell_ids: HashMap<String, CellId> = HashMap::new();
        for cell in &cells {
            let real_id = graph.add_cell(cell.clone());
            cell_ids.insert(cell.name.clone(), real_id);
        }
        graph.resolve_dependencies()?;
        let order = graph.topological_order()?;
        Self::print_success(None);

        // Build dependency map
        let deps: HashMap<CellId, Vec<CellId>> = cells
            .iter()
            .map(|cell| {
                let real_id = cell_ids[&cell.name];
                let dep_ids: Vec<CellId> = cell
                    .dependencies
                    .iter()
                    .filter_map(|dep| cell_ids.get(&dep.param_name).copied())
                    .collect();
                (real_id, dep_ids)
            })
            .collect();

        // Build universe
        Self::print_step("Building universe");
        let config = if release {
            CompilerConfig::for_notebook_release(&dirs)
        } else {
            CompilerConfig::for_notebook(&dirs)
        };

        let mut universe_builder =
            UniverseBuilder::for_notebook(config.clone(), toolchain.clone(), &abs_path)?;
        universe_builder.parse_dependencies(&source, &definition_cells)?;
        let universe_path = universe_builder.build()?;

        if universe_builder.dependencies().is_empty() {
            Self::print_success(Some("runtime only"));
        } else {
            Self::print_success(None);
        }

        Ok(Self {
            notebook_path: abs_path,
            source,
            dirs,
            toolchain,
            cells,
            cell_ids,
            order,
            deps,
            config,
            universe_builder,
            universe_path,
            release,
        })
    }
}
