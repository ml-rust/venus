//! Watch command implementation for Venus CLI.
//!
//! Watches a notebook for changes and auto-runs cells.

use std::path::Path;
use std::time::Instant;

use venus_server::{FileEvent, FileWatcher};

use crate::colors;
use crate::executor::NotebookExecutor;
use crate::output::print_output;

/// Execute the watch command.
pub async fn execute(
    notebook_path: &str,
    cell_filter: Option<&str>,
    release: bool,
    clear_screen: bool,
) -> anyhow::Result<()> {
    let path = Path::new(notebook_path);
    if !path.exists() {
        anyhow::bail!("Notebook not found: {}", notebook_path);
    }

    let abs_path = path.canonicalize()?;
    let notebook_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    // Print header
    println!(
        "\n{}Venus Watch{} - {}{}{}",
        colors::BOLD,
        colors::RESET,
        colors::CYAN,
        notebook_name,
        colors::RESET
    );
    println!("{}", "─".repeat(50));
    println!(
        "{}Watching for changes... (Ctrl+C to stop){}",
        colors::DIM,
        colors::RESET
    );
    println!();

    // Initial run
    if clear_screen {
        clear_terminal();
    }
    run_notebook(&abs_path, cell_filter, release)?;

    // Set up file watcher
    let mut watcher = FileWatcher::new(&abs_path)
        .map_err(|e| anyhow::anyhow!("Failed to create file watcher: {}", e))?;

    // Watch loop
    loop {
        match watcher.recv().await {
            Some(FileEvent::Modified(_)) => {
                println!(
                    "\n{}File changed, re-running...{}",
                    colors::YELLOW,
                    colors::RESET
                );

                if clear_screen {
                    clear_terminal();
                }

                if let Err(e) = run_notebook(&abs_path, cell_filter, release) {
                    eprintln!("{}Error:{} {}", colors::RED, colors::RESET, e);
                }
            }
            Some(FileEvent::Removed(path)) => {
                eprintln!(
                    "\n{}Warning:{} Notebook file removed: {}",
                    colors::YELLOW,
                    colors::RESET,
                    path.display()
                );
            }
            Some(FileEvent::Created(_)) => {
                println!(
                    "\n{}File recreated, re-running...{}",
                    colors::YELLOW,
                    colors::RESET
                );

                if clear_screen {
                    clear_terminal();
                }

                if let Err(e) = run_notebook(&abs_path, cell_filter, release) {
                    eprintln!("{}Error:{} {}", colors::RED, colors::RESET, e);
                }
            }
            None => break,
        }
    }

    Ok(())
}

/// Clear the terminal screen.
fn clear_terminal() {
    print!("\x1B[2J\x1B[1;1H");
    colors::flush_stdout();
}

/// Run the notebook once.
fn run_notebook(
    abs_path: &Path,
    cell_filter: Option<&str>,
    release: bool,
) -> anyhow::Result<()> {
    let start = Instant::now();

    // Create executor
    let executor = NotebookExecutor::new(abs_path.to_str().unwrap(), release)?;
    executor.print_header("Running");

    // Compile cells
    let compilation = executor.compile()?;

    // Execute cells
    let execution = executor.execute(&compilation, cell_filter)?;

    // Print outputs
    println!("\n{}Outputs:{}", colors::BOLD, colors::RESET);
    println!("{}", "─".repeat(50));

    for &cell_id in &execution.executed_cells {
        if let Some(cell) = executor.cell_by_id(cell_id)
            && let Some(output) = execution.outputs.get(&cell_id) {
                print_output(&cell.name, &cell.return_type, output.bytes());
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

    // Print watch status
    println!(
        "\n{}Watching for changes... (Ctrl+C to stop){}",
        colors::DIM,
        colors::RESET
    );

    Ok(())
}
