//! Build command implementation for Venus CLI.
//!
//! Compiles a notebook to a standalone binary.

use std::path::Path;
use std::time::Instant;

use venus_core::compile::{CompilerConfig, ProductionBuilder};
use venus_core::paths::NotebookDirs;

use crate::colors;

/// Result type for CLI operations.
pub type CliResult = anyhow::Result<()>;

/// Build a notebook to a standalone binary.
pub fn execute(notebook_path: &str, output: Option<&str>, release: bool) -> CliResult {
    let path = Path::new(notebook_path);
    if !path.exists() {
        anyhow::bail!(
            "Notebook not found: {} (current directory: {})",
            notebook_path,
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<unknown>".to_string())
        );
    }

    let start = Instant::now();
    let abs_path = path.canonicalize()?;

    println!(
        "\n{}Venus{} - Building {}{}{}\n",
        colors::BOLD,
        colors::RESET,
        colors::CYAN,
        path.file_name().unwrap_or_default().to_string_lossy(),
        colors::RESET
    );

    // Determine output path
    let output_path = if let Some(out) = output {
        Path::new(out).to_path_buf()
    } else {
        // Default: same name as notebook, in current directory
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .replace('-', "_");

        #[cfg(target_os = "windows")]
        let output_name = format!("{}.exe", name);

        #[cfg(not(target_os = "windows"))]
        let output_name = name;

        Path::new(&output_name).to_path_buf()
    };

    // Set up directories
    let dirs = NotebookDirs::from_notebook_path(&abs_path)?;
    let config = if release {
        CompilerConfig::for_notebook_release(&dirs)
    } else {
        CompilerConfig::for_notebook(&dirs)
    };

    // Load and parse notebook
    print!(
        "{}  ◆ Parsing notebook{} ... ",
        colors::BLUE,
        colors::RESET
    );
    colors::flush_stdout();

    let mut builder = ProductionBuilder::new(config);
    builder.load(&abs_path)?;

    println!(
        "{}✓{} ({} cells, {} dependencies)",
        colors::GREEN,
        colors::RESET,
        builder.cell_count(),
        builder.dependency_count()
    );

    // Build
    print!(
        "{}  ◆ Compiling binary{} ... ",
        colors::BLUE,
        colors::RESET
    );
    colors::flush_stdout();

    builder.build(&output_path, release)?;

    let duration = start.elapsed();

    println!("{}✓{}", colors::GREEN, colors::RESET);

    // Summary
    println!();
    println!(
        "{}Built:{} {}",
        colors::GREEN,
        colors::RESET,
        output_path.display()
    );
    println!(
        "{}Mode:{} {}",
        colors::DIM,
        colors::RESET,
        if release {
            "release (optimized)"
        } else {
            "debug"
        }
    );
    println!(
        "{}Time:{} {:.2}s",
        colors::DIM,
        colors::RESET,
        duration.as_secs_f64()
    );

    Ok(())
}
