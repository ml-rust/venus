//! Run command implementation for Venus CLI.
//!
//! Executes a notebook headlessly, compiling and running all cells.

use std::time::Instant;

use crate::colors;
use crate::executor::NotebookExecutor;
use crate::output::print_output;

/// Execute a notebook.
pub fn execute(
    notebook_path: &str,
    cell_filter: Option<&str>,
    release: bool,
) -> anyhow::Result<()> {
    let start = Instant::now();

    // Create executor (handles parsing, graph building, universe)
    let executor = NotebookExecutor::new(notebook_path, release)?;
    executor.print_header("Running");

    // Handle empty notebooks
    if executor.cells.is_empty() {
        println!(
            "\n{}No cells found in notebook.{}",
            colors::YELLOW,
            colors::RESET
        );
        println!("Cells are functions marked with #[venus::cell]");
        return Ok(());
    }

    // Compile cells
    let compilation = executor.compile()?;

    // Execute cells
    let execution = executor.execute(&compilation, cell_filter)?;

    // Print outputs
    println!("\n{}Outputs:{}", colors::BOLD, colors::RESET);
    println!("{}", "─".repeat(50));

    for &cell_id in &execution.executed_cells {
        if let Some(cell) = executor.cell_by_id(cell_id) {
            if let Some(output) = execution.outputs.get(&cell_id) {
                print_output(&cell.name, &cell.return_type, output.bytes());
            }
        }
    }

    // Summary
    let total_time = start.elapsed();
    println!("\n{}", "─".repeat(50));
    println!(
        "{}Completed{} {} cells in {:.2}s (execution: {:.2}s)",
        colors::GREEN,
        colors::RESET,
        execution.executed_cells.len(),
        total_time.as_secs_f64(),
        execution.execution_time.as_secs_f64()
    );

    Ok(())
}
