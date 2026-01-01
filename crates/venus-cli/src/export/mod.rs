//! Export module for Venus CLI.
//!
//! Generates standalone HTML files from notebook execution.

mod html;

pub use html::{generate_html, CellExport};

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use venus_core::graph::CellId;

use crate::colors;
use crate::executor::NotebookExecutor;
use crate::output::decoder::try_decode_value;

/// Execute a notebook and export to HTML.
pub fn execute(
    notebook_path: &str,
    output_path: Option<&str>,
    release: bool,
    dark_theme: bool,
) -> anyhow::Result<()> {
    let start = Instant::now();

    // Create executor (handles parsing, graph building, universe)
    let executor = NotebookExecutor::new(notebook_path, release)?;

    // Print header
    println!(
        "\n{}Venus Export{} - {}{}{}",
        colors::BOLD,
        colors::RESET,
        colors::CYAN,
        executor.notebook_name(),
        colors::RESET
    );
    println!("{}", "─".repeat(50));

    // Compile cells (silently for export - we'll print our own progress)
    println!("\n{}Compiling cells...{}", colors::BOLD, colors::RESET);

    let compilation = executor.compile()?;

    // Execute cells silently
    println!("\n{}Executing cells...{}", colors::BOLD, colors::RESET);

    // Build cell exports with execution results
    let mut cell_exports: HashMap<CellId, CellExport> = HashMap::new();

    // Initialize exports from cell info
    for cell in &executor.cells {
        let real_id = executor.cell_ids[&cell.name];

        // Check for compilation errors
        let error = compilation.errors.iter()
            .find(|(name, _)| name == &cell.name)
            .map(|(_, errs)| {
                errs.iter()
                    .map(|e| e.rendered.clone().unwrap_or_else(|| e.format_terminal()))
                    .collect::<Vec<_>>()
                    .join("\n")
            });

        cell_exports.insert(
            real_id,
            CellExport {
                name: cell.name.clone(),
                description: cell.doc_comment.clone(),
                source: cell.source_code.clone(),
                return_type: cell.return_type.clone(),
                dependencies: cell.dependencies.iter().map(|d| d.param_name.clone()).collect(),
                output: None,
                error,
                execution_time_ms: None,
            },
        );
    }

    // Execute cells if no compilation errors
    if compilation.errors.is_empty() {
        let execution = executor.execute_silent(&compilation, None)?;

        // Update exports with execution results
        for &cell_id in &execution.executed_cells {
            if let Some(cell) = executor.cell_by_id(cell_id) {
                if let Some(output) = execution.outputs.get(&cell_id) {
                    if let Some(export) = cell_exports.get_mut(&cell_id) {
                        // Use display_text if available, otherwise try to decode
                        let output_text = output
                            .display_text()
                            .map(|s| s.to_string())
                            .or_else(|| try_decode_value(&cell.return_type, output.bytes()));
                        export.output = output_text;
                        export.execution_time_ms =
                            Some(execution.execution_time.as_millis() as u64 / execution.executed_cells.len() as u64);
                    }
                }
            }
        }

        println!(
            "{}  ✓ Executed {} cells{} ({:.1}ms)",
            colors::GREEN,
            execution.executed_cells.len(),
            colors::RESET,
            execution.execution_time.as_secs_f64() * 1000.0
        );
    } else {
        println!(
            "{}  ⚠ Skipping execution ({} compilation errors){}",
            colors::YELLOW,
            compilation.errors.len(),
            colors::RESET
        );
    }

    // Generate HTML
    println!("\n{}Generating HTML...{}", colors::BOLD, colors::RESET);

    // Collect exports in execution order
    let ordered_exports: Vec<CellExport> = executor
        .order
        .iter()
        .filter_map(|id| cell_exports.remove(id))
        .collect();

    let html = generate_html(&executor.notebook_name(), &ordered_exports, dark_theme);

    // Determine output path
    let path = Path::new(notebook_path);
    let notebook_name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let output_file = output_path
        .map(|p| p.to_string())
        .unwrap_or_else(|| format!("{}.html", notebook_name));

    fs::write(&output_file, html)?;

    let total_time = start.elapsed();
    println!("{}", "─".repeat(50));
    println!(
        "{}Exported{} to {}{}{}",
        colors::GREEN,
        colors::RESET,
        colors::CYAN,
        output_file,
        colors::RESET
    );
    println!("Total time: {:.2}s", total_time.as_secs_f64());

    Ok(())
}
