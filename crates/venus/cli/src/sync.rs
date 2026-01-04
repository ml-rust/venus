//! Sync command implementation for Venus CLI.
//!
//! Converts .rs notebooks to .ipynb format.

use std::path::Path;
use std::time::Instant;

use venus_sync::{OutputCache, default_ipynb_path, sync_to_ipynb};

use crate::colors;

/// Execute the sync command.
pub fn execute(notebook_path: &str, watch: bool) -> anyhow::Result<()> {
    let path = Path::new(notebook_path);
    if !path.exists() {
        anyhow::bail!("Notebook not found: {}", notebook_path);
    }

    let abs_path = path.canonicalize()?;
    let ipynb_path = default_ipynb_path(&abs_path);

    println!(
        "\n{}Venus Sync{} - Converting to Jupyter format",
        colors::BOLD,
        colors::RESET
    );
    println!("{}", "─".repeat(50));

    // Set up output cache
    let notebook_dir = abs_path.parent().unwrap_or(Path::new("."));
    let cache_dir = notebook_dir.join(".venus").join("outputs");
    let cache = OutputCache::new(&cache_dir).ok();

    if watch {
        println!(
            "{}Watching{} {} for changes...",
            colors::CYAN,
            colors::RESET,
            path.display()
        );
        println!("Press Ctrl+C to stop.\n");

        // Initial sync
        sync_file(&abs_path, &ipynb_path, cache.as_ref())?;

        // Watch for changes using simple polling
        // TODO: Use notify crate for proper file watching
        watch_and_sync(&abs_path, &ipynb_path, cache.as_ref())?;
    } else {
        sync_file(&abs_path, &ipynb_path, cache.as_ref())?;
    }

    Ok(())
}

/// Sync a single file.
fn sync_file(rs_path: &Path, ipynb_path: &Path, cache: Option<&OutputCache>) -> anyhow::Result<()> {
    let start = Instant::now();

    print!(
        "  {} → {} ... ",
        rs_path.file_name().unwrap_or_default().to_string_lossy(),
        ipynb_path.file_name().unwrap_or_default().to_string_lossy()
    );
    std::io::Write::flush(&mut std::io::stdout()).ok();

    sync_to_ipynb(rs_path, ipynb_path, cache)?;

    let elapsed = start.elapsed();
    println!(
        "{}✓{} ({:.2}ms)",
        colors::GREEN,
        colors::RESET,
        elapsed.as_secs_f64() * 1000.0
    );

    Ok(())
}

/// Watch a file and sync on changes.
fn watch_and_sync(
    rs_path: &Path,
    ipynb_path: &Path,
    cache: Option<&OutputCache>,
) -> anyhow::Result<()> {
    use std::fs;
    use std::thread;
    use std::time::Duration;

    let mut last_modified = fs::metadata(rs_path)?.modified()?;

    loop {
        thread::sleep(Duration::from_millis(500));

        let current_modified = fs::metadata(rs_path)?.modified()?;

        if current_modified != last_modified {
            last_modified = current_modified;
            println!("\nFile changed, syncing...");

            if let Err(e) = sync_file(rs_path, ipynb_path, cache) {
                eprintln!("  Error: {}", e);
            }
        }
    }
}
