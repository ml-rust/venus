use super::{
    CompilationInfo, ExecutionInfo, NotebookExecutor, ProgressCallback, is_transitive_dependency,
};
use crate::colors;
use std::collections::HashMap;
use std::time::Instant;
use venus_core::execute::LinearExecutor;
use venus_core::graph::CellId;
use venus_core::state::StateManager;

impl NotebookExecutor {
    /// Execute cells with the given compilation info.
    ///
    /// If `cell_filter` is provided, only execute that cell and its dependencies.
    pub fn execute(
        &self,
        compilation: &CompilationInfo,
        cell_filter: Option<&str>,
    ) -> anyhow::Result<ExecutionInfo> {
        if !compilation.errors.is_empty() {
            println!(
                "\n{}Compilation failed for {} cell(s){}",
                colors::RED,
                compilation.errors.len(),
                colors::RESET
            );
            anyhow::bail!("Compilation failed");
        }

        println!("\n{}Executing cells...{}", colors::BOLD, colors::RESET);

        let state = StateManager::new(&self.dirs.state_dir)?;
        let mut executor = LinearExecutor::with_state(state);
        executor.set_callback(ProgressCallback::new());

        // Load all compiled cells
        for info in compilation.cells.values() {
            executor.load_cell(info.compiled.clone(), info.dep_count)?;
        }

        // Filter execution order if specific cell requested
        let execution_order = self.filter_execution_order(cell_filter)?;

        // Execute
        let exec_start = Instant::now();
        executor.execute_in_order(&execution_order, &self.deps)?;
        let execution_time = exec_start.elapsed();

        // Collect outputs
        let mut outputs = HashMap::new();
        for &cell_id in &execution_order {
            if let Some(output) = executor.state().get_output(cell_id) {
                outputs.insert(cell_id, output);
            }
        }

        Ok(ExecutionInfo {
            executed_cells: execution_order,
            execution_time,
            outputs,
        })
    }

    /// Execute cells without a callback (for export mode).
    pub fn execute_silent(
        &self,
        compilation: &CompilationInfo,
        cell_filter: Option<&str>,
    ) -> anyhow::Result<ExecutionInfo> {
        if !compilation.errors.is_empty() {
            anyhow::bail!("Compilation failed");
        }

        let state = StateManager::new(&self.dirs.state_dir)?;
        let mut executor = LinearExecutor::with_state(state);

        // Load all compiled cells
        for info in compilation.cells.values() {
            executor.load_cell(info.compiled.clone(), info.dep_count)?;
        }

        // Filter execution order if specific cell requested
        let execution_order = self.filter_execution_order(cell_filter)?;

        // Execute
        let exec_start = Instant::now();
        executor.execute_in_order(&execution_order, &self.deps)?;
        let execution_time = exec_start.elapsed();

        // Collect outputs
        let mut outputs = HashMap::new();
        for &cell_id in &execution_order {
            if let Some(output) = executor.state().get_output(cell_id) {
                outputs.insert(cell_id, output);
            }
        }

        Ok(ExecutionInfo {
            executed_cells: execution_order,
            execution_time,
            outputs,
        })
    }

    /// Filter execution order based on cell filter.
    fn filter_execution_order(&self, cell_filter: Option<&str>) -> anyhow::Result<Vec<CellId>> {
        if let Some(cell_name) = cell_filter {
            let target_id = self
                .cell_ids
                .get(cell_name)
                .ok_or_else(|| anyhow::anyhow!("Cell '{}' not found", cell_name))?;

            Ok(self
                .order
                .iter()
                .copied()
                .filter(|&id| {
                    id == *target_id || is_transitive_dependency(id, *target_id, &self.deps)
                })
                .collect())
        } else {
            Ok(self.order.clone())
        }
    }
}
