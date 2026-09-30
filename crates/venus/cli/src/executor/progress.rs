use crate::colors;
use venus_core::Error;
use venus_core::execute::ExecutionCallback;
use venus_core::graph::CellId;

/// Progress callback that prints execution status to the terminal.
pub struct ProgressCallback {
    /// Whether to show verbose output.
    verbose: bool,
}

impl ProgressCallback {
    /// Create a new progress callback.
    pub fn new() -> Self {
        Self { verbose: false }
    }

    /// Create a verbose progress callback.
    #[allow(dead_code)]
    pub fn verbose() -> Self {
        Self { verbose: true }
    }
}

impl Default for ProgressCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionCallback for ProgressCallback {
    fn on_cell_started(&self, _cell_id: CellId, name: &str) {
        print!(
            "{}  ▶ Running{} {}{}... ",
            colors::CYAN,
            colors::RESET,
            colors::BOLD,
            name
        );
        colors::flush_stdout();
    }

    fn on_cell_completed(&self, _cell_id: CellId, _name: &str) {
        println!("{}✓{}", colors::GREEN, colors::RESET);
    }

    fn on_cell_error(&self, _cell_id: CellId, _name: &str, error: &Error) {
        println!("{}✗{}", colors::RED, colors::RESET);
        eprintln!("{}    Error:{} {}", colors::RED, colors::RESET, error);
    }

    fn on_level_started(&self, level: usize, cell_count: usize) {
        if self.verbose && cell_count > 1 {
            println!(
                "{}Level {}:{} {} cells (parallel)",
                colors::DIM,
                level,
                colors::RESET,
                cell_count
            );
        }
    }

    fn on_level_completed(&self, _level: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_callback_creation() {
        let callback = ProgressCallback::new();
        assert!(!callback.verbose);

        let verbose = ProgressCallback::verbose();
        assert!(verbose.verbose);
    }
}
